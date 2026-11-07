//! 字节 → 文本：UTF-8 → CP932 → GBK，第一个**不出错**的解释胜出。
//!
//! 不能只用 `from_utf8_lossy`：日文站点下回来的 .lrc 有不少是 Shift-JIS，
//! 整份歌词会变成一串 U+FFFD。那比没有歌词更糟——库里记着「这首有歌词」，
//! 「补齐缺失歌词」再也不会看它一眼，而检索和制卡拿到的全是问号。
//!
//! CP932 排在 GBK 前面：两者的双字节区大面积重叠，同一串字节往往两边都能
//! 解出「字」来，只有一边是对的。这是个日文语料库，所以先假定日文编码；
//! 真有 GBK 的中文歌词时，它多半含 CP932 解不出的字节（实测
//! 「风的记忆」的 GBK 编码在 CP932 下第 17 字节就非法），于是会落到 GBK。
//!
//! **带 BOM 的 UTF-16（LE / BE）也走得通，但靠的是一个不显眼的行为**：
//! `Encoding::decode` 先嗅 BOM，看到 `FF FE` / `FE FF` 就改用 UTF-16 解，
//! 不管调用的是哪个编码（encoding_rs 文档写明的）。所以 CP932 那一步其实就把它解对了。
//! 改这段的时候别换成不嗅 BOM 的 `decode_without_bom_handling`——
//! `utf16_with_a_bom_is_decoded_both_ways` 会拦住。

use std::borrow::Cow;

/// 严格解码：三种编码都解不干净就是 None。结果去掉了 BOM。
pub fn decode_strict(bytes: &[u8]) -> Option<String> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(text.trim_start_matches('\u{feff}').to_string());
    }
    for encoding in [encoding_rs::SHIFT_JIS, encoding_rs::GBK] {
        let (text, _, had_errors) = encoding.decode(bytes);
        if !had_errors {
            return Some(text.trim_start_matches('\u{feff}').to_string());
        }
    }
    None
}

/// 同 [`decode_strict`]，但三种都不干净时按 UTF-8 尽力而为，坏字节变 U+FFFD。
/// 走到那一步的文件本身就是坏的，报错反而会让一首歌整个导不进来。
pub fn decode_bytes(bytes: &[u8]) -> String {
    decode_strict(bytes).unwrap_or_else(|| String::from_utf8_lossy(bytes).trim_start_matches('\u{feff}').to_string())
}

/// 只认 UTF-8 和带 BOM 的 UTF-16。分辨「这是文本还是密文」时用：
/// 随便一串密文字节在 CP932 / GBK 下也可能碰巧解得干净。
pub fn decode_unicode(bytes: &[u8]) -> Option<Cow<'_, str>> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(Cow::Borrowed(text));
    }
    let (encoding, _) = encoding_rs::Encoding::for_bom(bytes)?;
    let (text, _, had_errors) = encoding.decode(bytes);
    (!had_errors).then_some(text)
}

/// 读文件、解码成文本（去掉 BOM），不解析。编码规则同 [`decode_bytes`]。
pub fn read_file(path: &std::path::Path) -> anyhow::Result<String> {
    Ok(decode_bytes(&std::fs::read(path)?))
}
