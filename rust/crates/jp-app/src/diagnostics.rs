//! 「导出诊断信息」：把排查一个问题要问的那些话一次问完。
//!
//! 这一轮排查「改名失败」「振假名不可用」「有的歌点不动」，每一次都是
//! 截图 → 猜 → 让用户再试一次。要的其实就固定那几样：哪一版、库在哪、
//! 库里有多少东西、词典有没有、音频行不行、最近报了什么错。
//!
//! 所以做成一段纯文本：用户点一下存成文件或者复制走，贴给我就行。
//! **不联网、不自动上报**——要发什么、发给谁，是用户的事。
//!
//! 刻意不收集的：曲名、歌手、歌词、文件名。路径里会带用户名（库就在
//! `C:\Users\<名字>\…` 下面），这一点在界面上写明白了。

use std::fmt::Write as _;

use crate::state::AppState;

/// 攒一份诊断文本。
pub fn report(state: &AppState, version: &str, log_lines: usize) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "JPOP Corpus Tool 诊断信息");
    let _ = writeln!(out, "时间（UTC）  {}", crate::log::utc_now());
    let _ = writeln!(out, "版本        {version}");
    let _ = writeln!(
        out,
        "系统        {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    let root = state.project_root();
    let _ = writeln!(out, "\n── 语料库 ──");
    let _ = writeln!(out, "目录        {}", root.display());
    let _ = writeln!(out, "corpus.db   {}", state.db_path.display());
    match state.corpus().overview() {
        Ok(overview) => {
            let _ = writeln!(
                out,
                "曲目 {} · 歌词行 {} · 词汇 {}",
                overview.tracks, overview.lyric_lines, overview.vocabulary
            );
        }
        Err(err) => {
            let _ = writeln!(out, "读不出总览：{err:#}");
        }
    }
    // 有歌词没分词的那些——「点词查不了、没有振假名」就是这个状态
    match jp_import::maintain::untokenized_songs(state.corpus().connection()) {
        Ok(songs) if songs.is_empty() => {
            let _ = writeln!(out, "有歌词没分词 0 首");
        }
        Ok(songs) => {
            let _ = writeln!(
                out,
                "有歌词没分词 {} 首（设置 → 曲库维护 → 补齐缺失分词）",
                songs.len()
            );
        }
        Err(err) => {
            let _ = writeln!(out, "查不出缺分词的歌：{err:#}");
        }
    }

    let _ = writeln!(out, "\n── 能力 ──");
    let dict = crate::tokenizer::status(&root);
    let _ = writeln!(
        out,
        "分词词典    {}（{}，{:.0} MB）",
        if dict.ready { "有" } else { "没有" },
        dict.source,
        dict.dict_bytes as f64 / 1024.0 / 1024.0
    );
    if let Some(path) = &dict.dict_path {
        let _ = writeln!(out, "            {path}");
    }
    let _ = writeln!(
        out,
        "音频输出    {}",
        if state.audio().is_some() { "有" } else { "没有" }
    );
    let _ = writeln!(
        out,
        "变调        {}",
        if state.pitch().available() {
            "支持"
        } else {
            "不支持（缺 ffmpeg）"
        }
    );
    let _ = writeln!(
        out,
        "词典库      {}",
        if root.join("dictionaries.db").is_file() {
            "有"
        } else {
            "没有"
        }
    );

    let _ = writeln!(out, "\n── 日志（最后 {log_lines} 行）──");
    let _ = writeln!(out, "文件 {}", crate::log::log_path(&root).display());
    let lines = crate::log::tail(log_lines);
    if lines.is_empty() {
        let _ = writeln!(out, "（这次启动还没写下任何东西）");
    } else {
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
    }
    out
}
