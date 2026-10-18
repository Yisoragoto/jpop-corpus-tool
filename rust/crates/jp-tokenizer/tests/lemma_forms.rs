//! 词云合并异体写法用的 `lemma_forms`。要真词典：CI 上经 `JP_SUDACHI_SOURCE` 拿到，本机没有就跳过。

use std::path::PathBuf;

use jp_tokenizer::{find_sudachipy_source, Analyzer};

fn analyzer() -> Option<Analyzer> {
    let source = std::env::var("JP_SUDACHI_SOURCE").ok().filter(|s| !s.is_empty())?;
    let (resources, dict) = find_sudachipy_source(&PathBuf::from(source))?;
    Some(Analyzer::from_sudachipy(&resources, &dict).expect("加载词典失败"))
}

#[test]
fn variant_spellings_share_a_normalized_form() {
    let Some(a) = analyzer() else {
        eprintln!("跳过：没有 JP_SUDACHI_SOURCE");
        return;
    };
    let lemmas: Vec<String> = ["思い出", "想い出", "夜"].iter().map(|s| s.to_string()).collect();
    let forms = a.lemma_forms(&lemmas);
    assert_eq!(forms.len(), 3);
    assert_eq!(forms[0].normalized, forms[1].normalized, "{forms:?}");
    assert_eq!(forms[0].reading, "おもいで");
    assert_eq!(forms[2].reading, "よる");
}

#[test]
fn compounds_are_left_alone() {
    let Some(a) = analyzer() else {
        eprintln!("跳过：没有 JP_SUDACHI_SOURCE");
        return;
    };
    // 分成好几段的不算认准：原样返回、读音为空
    let forms = a.lemma_forms(&["夏風邪薬局前".to_string()]);
    assert_eq!(forms[0].normalized, "夏風邪薬局前");
    assert_eq!(forms[0].reading, "");
}

/// 本机看效果：`LEMMAS=词表文件 cargo test -p jp-tokenizer --test lemma_forms probe -- --ignored --nocapture`
#[test]
#[ignore]
fn probe() {
    let a = analyzer().expect("要 JP_SUDACHI_SOURCE");
    let text = std::fs::read_to_string(std::env::var("LEMMAS").expect("要 LEMMAS")).unwrap();
    let lemmas: Vec<String> = text.lines().map(str::to_string).filter(|l| !l.is_empty()).collect();
    for f in a.lemma_forms(&lemmas) {
        if f.normalized != f.lemma {
            println!("{}\t{}\t{}", f.lemma, f.normalized, f.reading);
        }
    }
}
