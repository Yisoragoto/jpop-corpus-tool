use super::*;

#[test]
fn offline_status_and_errors_use_an_injected_transport() {
    use jp_scraper::ProviderError;
    use jp_scraper::http::{HttpClient, HttpResponse, Timeouts, Transport};
    struct Offline;
    impl Transport for Offline {
        fn get(
            &self,
            _: &str,
            _: &[(String, String)],
            _: Timeouts,
        ) -> std::result::Result<HttpResponse, ProviderError> {
            Err(ProviderError::network("离线测试"))
        }
        fn post(
            &self,
            _: &str,
            _: &[u8],
            _: &[(String, String)],
            _: Timeouts,
        ) -> std::result::Result<HttpResponse, ProviderError> {
            Err(ProviderError::network("离线测试"))
        }
    }
    let (s, dir) = tests::session();
    let client = jp_anki::AnkiConnect::new(HttpClient::new(Box::new(Offline)));
    let status = anki::dispatch_with_client(&s, "ankiStatus", &json!({}), &client)
        .unwrap()
        .unwrap();
    assert_eq!(status["connected"], false);
    assert!(status["message"].as_str().unwrap().contains("2055492159"));
    for key in [
        "version",
        "decks",
        "noteTypeReady",
        "collectionPath",
        "knownWords",
        "studiedWords",
    ] {
        assert!(status.get(key).is_some(), "{key}");
    }
    assert_eq!(
        anki::dispatch_with_client(&s, "ankiModelNames", &json!({}), &client)
            .unwrap()
            .unwrap(),
        json!([])
    );
    for (method, args) in [
        ("ankiEnsureNoteType", json!({})),
        ("ankiBrowseNotes", json!({"noteIds":[1]})),
        ("ankiRefreshPreview", json!({"deck":"D"})),
    ] {
        let error = anki::dispatch_with_client(&s, method, &args, &client).unwrap_err();
        assert!(error.to_string().contains("连不上 Anki"), "{error}");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn offline_words_preview_and_invalid_requests() {
    let (s, dir) = tests::session();
    let words = s
        .call("ankiWords", json!({"limit":300,"skipStudied":true}))
        .unwrap();
    assert!(!words.as_array().unwrap().is_empty());
    let word = &words[0];
    for key in ["lemma", "pos", "count", "songCount", "inAnki", "studied"] {
        assert!(word.get(key).is_some(), "{key}");
    }
    let card = s
        .call(
            "ankiPreview",
            json!({"lemma":word["lemma"], "pos":word["pos"], "maxExamples":0}),
        )
        .unwrap();
    assert!(!card["noExamples"].as_bool().unwrap());
    assert_eq!(card["examples"].as_array().unwrap().len(), 1);
    let empty = s
        .call("ankiPreview", json!({"lemma":"不存在的词"}))
        .unwrap();
    assert_eq!(empty["noExamples"], true);
    assert_eq!(empty["noDefinitions"], true);
    for method in ["ankiExportStart", "ankiUpdateStart"] {
        assert!(
            s.call(method, json!({"words":[],"deck":"D"}))
                .unwrap_err()
                .to_string()
                .contains("没有选中")
        );
    }
    assert!(
        s.call("ankiRefreshStart", json!({"deck":" "}))
            .unwrap_err()
            .to_string()
            .contains("牌组")
    );
    assert_eq!(s.call("ankiIsRunning", json!({})).unwrap(), false);
    s.call("ankiCancel", json!({})).unwrap();
    assert_eq!(s.call("ankiProgress", json!({})).unwrap()["running"], false);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn cancelled_batch_is_pollable_without_http_or_session_lock() {
    let (s, dir) = tests::session();
    let db = s.db_path.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let lock = s.corpus.lock().unwrap();
    s.anki_job
        .spawn("Anki 作业", move |job| {
            rx.recv().unwrap();
            let p = jp_anki::batch::run_export(
                &db,
                &jp_anki::AnkiConnect::local(),
                &[],
                &jp_anki::ExportOptions::default(),
                || job.cancelled(),
                |_| panic!("不应发送进度"),
            );
            Ok((json!({"progress":p,"log":[]}), p.cancelled))
        })
        .unwrap();
    s.call("ankiCancel", json!({})).unwrap();
    assert!(s.anki_job.spawn("Anki 作业", |_| unreachable!()).is_err());
    tx.send(()).unwrap();
    for _ in 0..500 {
        let p = s.call("ankiProgress", json!({})).unwrap();
        if p["finished"] == true {
            assert_eq!(p["cancelled"], true);
            assert_eq!(p["result"]["progress"]["job"], "export");
            assert_eq!(p["result"]["progress"]["finished"], true);
            drop(lock);
            std::fs::remove_dir_all(dir).ok();
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("作业没结束");
}

#[test]
fn text_and_png_exports_validate_and_read_back() {
    let (s, dir) = tests::session();
    let path = dir.join("report.txt");
    let report = s.call("statsReport", json!({})).unwrap();
    s.call("exportText", json!({"path":path,"content":report["text"]}))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        report["text"].as_str().unwrap()
    );
    assert!(
        s.call(
            "exportText",
            json!({"path":dir.join("bad.exe"),"content":"x"})
        )
        .is_err()
    );
    assert!(
        s.call("exportPng", json!({"path":dir.join("bad.png"),"bytes":[0]}))
            .is_err()
    );
    let png = b"\x89PNG\r\n\x1a\n";
    s.call("exportPng", json!({"path":dir.join("ok.png"),"bytes":png}))
        .unwrap();
    assert_eq!(std::fs::read(dir.join("ok.png")).unwrap(), png);
    std::fs::remove_dir_all(dir).ok();
}
