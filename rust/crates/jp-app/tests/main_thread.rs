//! 哪些 command 允许跑在主线程上。
//!
//! Tauri 2：「Commands without the async keyword are executed on the main thread unless
//! defined with `#[tauri::command(async)]`」（v2.tauri.app/develop/calling-rust）。
//! 主线程就是窗口的消息循环——命令在那儿多跑一秒，整个窗口就冻一秒。
//!
//! 冻住的不只是「慢查询」：主线程上的命令只要去拿 `corpus()` 锁，而后台作业
//! （导入、补分词、迁移）正持着锁跑长事务，窗口就一直卡到事务结束。
//!
//! 所以规矩是：**碰库、碰文件、碰网络、或者可能等锁的，一律不许是同步命令**。
//! 留在主线程的只有下面这张白名单——纯内存、微秒级。新加的同步命令不在表里，这条测试就红。

/// 允许同步（跑在主线程）的命令，以及为什么允许。
const MAIN_THREAD_OK: &[(&str, &str)] = &[
    ("audio_play", "只动播放器和变调状态里的一个标志"),
    ("audio_pause", "同上"),
    ("audio_toggle", "同上"),
    ("audio_stop", "同上"),
    ("audio_set_rate", "播放器的一个参数"),
    ("audio_set_volume", "播放器的一个参数"),
    ("audio_set_loop", "播放器的一个参数"),
    ("audio_state", "读播放器状态，不碰库"),
    ("audio_spectrum", "内存里做一次 FFT，几百微秒"),
    ("lyrics_fill_cancel", "置一个取消标志"),
    ("lyrics_fill_running", "读一个标志"),
    ("scrape_cancel", "置一个取消标志"),
    ("scrape_is_running", "读一个标志"),
    ("anki_cancel", "置一个取消标志"),
    ("anki_is_running", "读一个标志"),
    ("dict_import_cancel", "置一个取消标志"),
    ("dict_is_importing", "读一个标志"),
    ("cancel_import", "置取消标志、清掉内存里的扫描结果"),
];

/// 源码里所有 `#[tauri::command]`：(名字, 是否同步)。
fn commands() -> Vec<(String, bool)> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let attr = line.trim();
            if !attr.starts_with("#[tauri::command") {
                continue;
            }
            // 属性和 fn 之间可能隔着别的属性或注释
            let Some(sig) = lines[i + 1..].iter().map(|l| l.trim()).find(|l| l.starts_with("pub ")) else {
                continue;
            };
            let is_async_fn = sig.starts_with("pub async fn ");
            let threadpool = attr.contains("async");
            let name = sig
                .trim_start_matches("pub async fn ")
                .trim_start_matches("pub fn ")
                .split(['<', '('])
                .next()
                .unwrap()
                .to_string();
            out.push((name, !is_async_fn && !threadpool));
        }
    }
    out
}

#[test]
fn only_whitelisted_commands_run_on_the_main_thread() {
    let all = commands();
    assert!(all.len() > 100, "只扫到 {} 个命令，扫描本身出问题了", all.len());
    let offenders: Vec<&str> = all
        .iter()
        .filter(|(name, sync)| *sync && !MAIN_THREAD_OK.iter().any(|(ok, _)| ok == name))
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        offenders.is_empty(),
        "这些命令跑在主线程上，却会碰库 / 文件 / 网络 / 锁：{offenders:?}\n\
         改成 async，或者加 #[tauri::command(async)]；真是纯内存的，加进 MAIN_THREAD_OK 并写明理由"
    );
}

/// 白名单里的名字必须真实存在，而且真是同步的——改名或者已经改成 async 之后要从表里删掉，
/// 否则这张表会慢慢变成「看上去在管、其实什么都不管」
#[test]
fn the_whitelist_has_no_stale_entries() {
    let all = commands();
    for (name, _) in MAIN_THREAD_OK {
        match all.iter().find(|(n, _)| n == name) {
            None => panic!("白名单里的 {name} 不存在了"),
            Some((_, false)) => panic!("{name} 已经不是同步命令了，从白名单里删掉"),
            Some(_) => {}
        }
    }
}
