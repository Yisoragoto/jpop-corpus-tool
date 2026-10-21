use std::{fs, path::PathBuf, process::{Command, Output}};

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("jp-cli-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("local/JPOP Corpus Tool")).unwrap();
        fs::create_dir_all(root.join("library")).unwrap();
        fs::write(root.join("local/JPOP Corpus Tool/settings.json"), serde_json::json!({"libraryRoot":root.join("library")}).to_string()).unwrap();
        Self(root)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_jp")).args(args).current_dir(&self.0)
            .env_remove("JPOP_CORPUS_HOME").env("LOCALAPPDATA", self.0.join("local")).output().unwrap()
    }
}

impl Drop for Sandbox { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

#[test]
fn scanning_an_empty_directory_is_json_and_does_not_create_a_database() {
    let sandbox = Sandbox::new();
    let result = sandbox.run(&["scan", sandbox.0.to_str().unwrap(), "--json"]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let json: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(json["summary"]["total"], 0);
    assert!(!sandbox.0.join("library/corpus.db").exists());
}

#[test]
fn info_builds_an_empty_library_and_reports_its_counts() {
    let sandbox = Sandbox::new();
    let result = sandbox.run(&["info"]);
    assert!(result.status.success());
    let corpus = jp_corpus::Corpus::open(sandbox.0.join("library/corpus.db")).unwrap();
    assert_eq!(corpus.overview().unwrap().tracks, 0);
    assert_eq!(jp_corpus::migrations::version(corpus.connection()).unwrap(), jp_corpus::migrations::CURRENT);
    assert!(String::from_utf8(result.stdout).unwrap().contains('0'));
}

#[test]
fn usage_and_runtime_errors_have_different_exit_codes() {
    let sandbox = Sandbox::new();
    for args in [&["unknown"][..], &["scan"], &["info", "--json"], &["import", "--typo"], &["tokenize", "--typo"], &["export-anki", "out.tsv"]] {
        assert_eq!(sandbox.run(args).status.code(), Some(1), "{args:?}");
    }
    assert_eq!(sandbox.run(&["scan", "missing"]).status.code(), Some(2));
    assert_eq!(sandbox.run(&["--help"]).status.code(), Some(0));
}

#[test]
fn exporting_a_seeded_library_contains_real_card_fields() {
    let sandbox = Sandbox::new();
    jp_corpus::fixture::write_to(&sandbox.0.join("library")).unwrap();
    let out = sandbox.0.join("cards.tsv");
    let result = sandbox.run(&["export-anki", "--out", out.to_str().unwrap()]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let text = fs::read_to_string(out).unwrap();
    assert!(text.contains("\"夜\""), "导出的不只是表头：{text}");
    assert!(text.contains(">夜</b>が明ける前に"), "应包含标出目标词的语料例句：{text}");
}

#[test]
fn importing_a_file_and_exporting_cards_reuse_the_same_library() {
    let sandbox = Sandbox::new();
    let audio = sandbox.0.join("Artist - Example.wav");
    let pcm = vec![0_u8; 16000];
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    wav.extend_from_slice(&8000_u32.to_le_bytes());
    wav.extend_from_slice(&16000_u32.to_le_bytes());
    wav.extend_from_slice(b"\x02\0\x10\0data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    fs::write(&audio, wav).unwrap();
    fs::write(audio.with_extension("lrc"), "[00:00.00]CLI fixture\n").unwrap();
    let result = sandbox.run(&["import", audio.to_str().unwrap(), "--json"]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let corpus = jp_corpus::Corpus::open(sandbox.0.join("library/corpus.db")).unwrap();
    assert_eq!(corpus.overview().unwrap().tracks, 1);
    assert_eq!(corpus.overview().unwrap().lyric_lines, 1);
    let fields = jp_corpus::identity::fields(corpus.connection(),"001").unwrap();
    assert_eq!(fields["audio_fingerprint"].len(),64);
    let rescan = sandbox.run(&["fingerprint"]);
    assert!(rescan.status.success(),"{}",String::from_utf8_lossy(&rescan.stderr));
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&rescan.stdout).unwrap()["updated"],1);
    assert_eq!(jp_corpus::identity::fields(corpus.connection(),"001").unwrap()["audio_fingerprint"],fields["audio_fingerprint"]);
    let plan = sandbox.run(&["scan", audio.to_str().unwrap(), "--json"]);
    let json: serde_json::Value = serde_json::from_slice(&plan.stdout).unwrap();
    assert_eq!(json["summary"]["alreadyImported"], 1);
    let out = sandbox.0.join("cards.tsv");
    assert!(sandbox.run(&["export-anki", "--out", out.to_str().unwrap()]).status.success());
    assert!(fs::read_to_string(out).unwrap().contains("#separator:tab"));
}
