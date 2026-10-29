//! JPOP Corpus Tool 的 Tauri 壳。
//!
//! 这一层只做三件事：定位项目根目录、装配状态、注册 command。
//! 所有业务逻辑在 `jp-corpus` / `jp-tokenizer` 里——那两层能离线测试，
//! 这一层不能。

// 公开给集成测试用：`tests/commands.rs` 要能装配同一套状态和 handler。
// 生产路径和测试路径共用 `register()`，命令清单只有一份。
pub mod anki;
pub mod anki_report;
pub mod commands;
pub mod diagnostics;
pub mod dict;
pub mod fonts;
pub mod job;
pub mod library_admin;
pub mod log;
pub mod lyrics;
pub mod timed_lyrics;
pub mod lyric_upgrade;
pub mod maintenance;
pub mod migrate;
/// 断点续传下载。和 Tauri 无关，在 `jp-setup` 里，macOS 客户端也用它
pub use jp_setup::net;
pub mod mine;
pub mod pitch;
pub mod scrape;
pub mod state;
/// 分词词典放在哪、怎么补上。和 Tauri 无关，在 `jp-setup` 里，macOS 客户端也用它
pub use jp_setup::sudachi as tokenizer;
pub mod tracker;
pub mod update;


use state::AppState;

pub mod library_root;
pub mod e2e;

/// 注册全部 command。
///
/// 抽出来是为了让集成测试用 `tauri::test::mock_builder()` 装配**同一份清单**——
/// 测试和生产各写一份的话，测试通过也证明不了生产注册对了。
pub fn register<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    macro_rules! handler {
        ($builder:expr $(, $extra:path)*) => {
            $builder.invoke_handler(tauri::generate_handler![
                $($extra,)*
            commands::health,
            commands::update_preferences,
            commands::update_set_channel,
            commands::update_skip,
            commands::library_root,
            commands::set_library_root,
            // 曲库
            commands::list_tracks,
            commands::get_track,
            commands::list_albums,
            commands::album_tracks,
            commands::track_credits,
            commands::people_by_role,
            commands::people_directory,
            commands::person_works,
            commands::collaboration_edges,
            commands::timeline_releases,
            commands::person_by_id,
            commands::works_by_person,
            commands::collaborators,
            commands::lyrics,
            // 检索
            commands::kwic,
            commands::line_spans,
            commands::listen_window,
            commands::corpus_snapshot,
            commands::corpus_frequency,
            commands::collocations,
            commands::keyness,
            commands::ngrams,
            commands::lexical_profile,
            commands::inflection,
            commands::orthography,
            commands::song_profile,
            commands::search_lyrics,
            commands::quick_search,
            // 语料
            commands::overview,
            commands::timeline,
            commands::word_frequency,
            commands::word_in_corpus,
            // 播放历史
            commands::record_play,
            commands::recently_played,
            commands::most_played,
            // 播放
            commands::audio_load,
            commands::audio_play,
            commands::audio_pause,
            commands::audio_toggle,
            commands::audio_stop,
            commands::audio_seek,
            commands::audio_set_rate,
            commands::audio_set_volume,
            commands::audio_set_loop,
            commands::audio_set_stop_at,
            commands::audio_set_pitch,
            commands::pitch_cache_status,
            commands::pitch_cache_set_limit,
            commands::lyrics_furigana,
            commands::lemma_forms,
            commands::word_timings,
            commands::word_time_set,
            commands::word_time_clear,
            commands::fonts_catalog,
            commands::fonts_import_archive,
            commands::library_edit_song,
            commands::library_delete_song,
            commands::library_missing_audio,
            commands::library_suggest_relinks,
            commands::library_relink_audio,
            commands::stats_frequency,
            commands::stats_report,
            commands::export_text,
            commands::export_png,
            commands::memberships,
            commands::band_members,
            commands::bands_of,
            commands::membership_put,
            commands::membership_confirm,
            commands::membership_reject,
            commands::membership_suggestions,
            commands::anki_mining_report,
            commands::audio_state,
            commands::audio_tick,
            commands::audio_spectrum,
            // 首页 / 收藏
            commands::home_summary,
            commands::toggle_favorite,
            commands::is_favorite,
            // 维护
            commands::backfill_durations,
            commands::migrate_plan,
            commands::migrate_run,
            commands::diagnostics_report,
            commands::diagnostics_save,
            commands::tokenizer_status,
            commands::tokenizer_install,
            commands::tokenizer_download,
            commands::tokenize_missing_list,
            commands::tokenize_missing_run,
            // 检查更新
            commands::update_check,
            commands::update_changelog,
            commands::update_download,
            commands::update_install,
            commands::lyrics_missing,
            commands::lyrics_fill_start,
            commands::lyrics_fill_cancel,
            commands::lyrics_fill_running,
            commands::lyrics_fill_one,
            commands::lyrics_upgrade_start,
            commands::lyrics_upgrade_status,
            commands::lyrics_upgrade_cancel,
            commands::lyrics_timed_targets,
            commands::lyrics_timed_search,
            commands::lyrics_timed_apply,
            commands::lyrics_sources_get,
            commands::lyrics_sources_set,
            commands::lyrics_import_file,
            // 语料质量与来源追踪
            commands::quality_overview,
            commands::song_lineage,
            commands::artist_provenance,
            commands::lyric_version_raw,
            commands::lyrics_restore_version,
            commands::lyrics_save_lines,
            commands::lyrics_save_timing,
            commands::audio_peaks,
            commands::lyrics_revalidate,
            commands::song_set_language,
            // 子语料与可复现分析
            commands::subcorpus_facets,
            commands::subcorpus_preview,
            commands::subcorpus_list,
            commands::subcorpus_save,
            commands::subcorpus_delete,
            commands::analysis_run,
            commands::analysis_list,
            commands::analysis_get,
            commands::analysis_rerun,
            commands::analysis_compare,
            commands::analysis_delete,
            // 分词
            commands::tokenize,
            // 导入：扫描 → 复核 → 确认
            commands::scan_folder,
            commands::scan_files,
            commands::run_import,
            commands::cancel_import,
            commands::library_new_audio,
            // 刮削：识别 / 补 metadata / 封面
            commands::scrape_summary,
            commands::scrape_review_queue,
            commands::scrape_attempts,
            commands::scrape_track,
            commands::scrape_start,
            commands::scrape_album_fill_start,
            commands::scrape_cancel,
            commands::scrape_is_running,
            commands::scrape_accept,
            commands::scrape_skip,
            commands::scrape_retry_failed,
            commands::artist_roster,
            commands::library_completeness,
            commands::scrape_artist,
            commands::artist_image_set,
            commands::artist_image_reset,
            commands::scrape_artists_start,
            commands::fill_album_artwork,
            commands::scrape_backfill_covers,
            // Anki：选词 / 预览 / 导出 / 学习状态
            commands::anki_status,
            commands::anki_notes_since,
            commands::anki_words,
            commands::anki_preview,
            commands::anki_ensure_note_type,
            commands::anki_export_start,
            commands::anki_cancel,
            commands::anki_is_running,
            commands::anki_update_start,
            commands::anki_refresh_preview,
            commands::anki_refresh_start,
            commands::token_correction,
            commands::save_token_correction,
            commands::revert_token_correction,
            commands::dict_list,
            commands::dict_legacy_sources,
            commands::dict_import_start,
            commands::dict_import_cancel,
            commands::dict_is_importing,
            commands::dict_set_enabled,
            commands::dict_set_order,
            commands::dict_delete,
            commands::dict_lookup,
            commands::dict_media,
            commands::dict_styles,
            commands::anki_mine,
            commands::anki_mine_check,
            commands::anki_browse_notes,
            commands::anki_model_names,
            ])
        };
    }
    let builder = builder.plugin(tauri_plugin_dialog::init());
    if e2e::enabled(std::env::var("JPOP_E2E").ok().as_deref()) {
        handler!(builder, e2e::e2e_state, e2e::e2e_scan_folder)
    } else {
        handler!(builder)
    }
}

pub fn run() {
    let (root, source) = library_root::locate();
    // 日志先开：下面任何一步出事都要留下痕迹，尤其是「启动失败」那一条——
    // 用户那边只看得到一个对话框，我这边只有这个文件
    crate::log::init(&root);
    jp_lyrics::set_logger(|level, message| match level {
        jp_lyrics::LogLevel::Warn => crate::log::warn(message),
        jp_lyrics::LogLevel::Error => crate::log::error(message),
    });
    crate::scrape::connect_log();
    crate::log::info(format!(
        "启动 {} · 语料库目录：{}（来自 {source:?}）",
        env!("CARGO_PKG_VERSION"),
        root.display()
    ));
    let state = match AppState::new(&root) {
        Ok(state) => state,
        Err(err) => {
            // 数据库打不开就没有任何功能可谈。但**不能默不作声地退出**：
            // 双击桌面图标的人看不到 stderr，表现就是「点了没反应」。
            let message = format!(
                "启动失败：{err:#}\n\n数据目录：{}\n\n                 已有的语料库可以用环境变量 JPOP_CORPUS_HOME 指过去（指向含 corpus.db 的目录）。",
                root.display()
            );
            crate::log::error(&message);
            show_fatal_error(&message);
            std::process::exit(1);
        }
    };

    let media = media_dirs(&state);

    let app = register(tauri::Builder::default())
        .setup(move |app| {
            allow_media_dirs(app.handle(), &media);
            // 存量 token 的形态信息（20261029 起有的五列）在后台补，不挡启动
            commands::spawn_annotate_backlog(app.handle().clone());
            #[cfg(all(windows, target_pointer_width = "64"))]
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main")
                    && let Ok(hwnd) = window.hwnd()
                {
                    backdrop::keep_active(hwnd);
                }
            }
            Ok(())
        })
        .manage(state)
        .build(tauri::generate_context!())
        .expect("Tauri 装配失败");

    app.run(|handle, event| {
        // 退出前把最后一段收听写进历史，否则关窗口那首歌就白听了
        if matches!(event, tauri::RunEvent::Exit) {
            use tauri::Manager;
            handle.state::<AppState>().flush_session();
        }
    });
}

/// 窗口失焦时也保持系统亚克力背景。
///
/// Windows 11 只在窗口激活时画亚克力，失焦（点了别的窗口、弹出系统对话框）
/// 就换成一块灰色纯色，界面底色跟着一亮一暗，看着就是背景在闪。
/// `windowEffects.state` 只管 macOS。系统按非客户区的激活状态决定画哪种，
/// 所以把 `WM_NCACTIVATE` 的参数一律改成「激活」：背景不再切换，
/// 键盘焦点照常转移（那走 WM_ACTIVATE / WM_SETFOCUS）。代价是标题栏失焦时不变灰。
#[cfg(all(windows, target_pointer_width = "64"))]
mod backdrop {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, GWLP_WNDPROC, SetWindowLongPtrW, WM_NCACTIVATE, WNDPROC,
    };

    /// 原来的窗口过程。只有一个主窗口，存一个就够
    static PREVIOUS: AtomicIsize = AtomicIsize::new(0);

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let wparam = if msg == WM_NCACTIVATE {
            WPARAM(1)
        } else {
            wparam
        };
        // SAFETY: PREVIOUS 在换上本函数之前就存好了，是 SetWindowLongPtrW 返回的原窗口过程
        unsafe {
            let previous: WNDPROC = std::mem::transmute(PREVIOUS.load(Ordering::Acquire));
            CallWindowProcW(previous, hwnd, msg, wparam, lparam)
        }
    }

    pub fn keep_active(hwnd: HWND) {
        // SAFETY: hwnd 是本进程刚建好的主窗口；先存原过程再换，换上之后的消息都能转回去
        unsafe {
            let previous =
                windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
            if previous == 0 {
                return;
            }
            PREVIOUS.store(previous, Ordering::Release);
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, window_proc as *const () as isize);
        }
    }
}

/// 启动失败时弹一个系统对话框。
///
/// GUI 程序没有控制台，`eprintln!` 等于什么都没说——用户看到的是「双击没反应」。
#[cfg(windows)]
fn show_fatal_error(message: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::PCWSTR;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let text = wide(message);
    let title = wide("JPOP Corpus Tool");
    // SAFETY: 两个指针都指向以 0 结尾的 UTF-16 缓冲，活到调用结束
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_fatal_error(_message: &str) {}

/// 界面要直接显示的本地图片目录：刮削下来的曲目封面和歌手照片。
///
/// 两个目录缺一不可——只放行封面的时候，人物页一张照片都出不来，而且不报错。
pub fn media_dirs(state: &AppState) -> Vec<std::path::PathBuf> {
    vec![state.covers_dir.clone(), state.artists_dir.clone()]
}

/// 把这些目录加进 asset 协议的放行范围。
///
/// 图片是磁盘上的普通文件，webview 默认碰不到。只放行这几个目录——项目根可以用
/// `JPOP_CORPUS_HOME` 换，所以运行时授权，不写死在 tauri.conf.json 里。
/// 目录还不存在也先建出来再授权：用户可能先开界面再刮削。
///
/// 授权失败只打日志：图显示不了不该拦住整个程序。
pub fn allow_media_dirs<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
    dirs: &[std::path::PathBuf],
) {
    use tauri::Manager;
    let scope = handle.asset_protocol_scope();
    for dir in dirs {
        let _ = std::fs::create_dir_all(dir);
        if let Err(err) = scope.allow_directory(dir, true) {
            eprintln!("图片目录授权失败（{}）：{err}", dir.display());
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn locate_project_root_returns_something() {
        // 找不到时也要给出一个可用的路径，不能 panic
        let (root, _source) = library_root::locate();
        assert!(!root.as_os_str().is_empty());
    }

    #[test]
    fn env_override_wins_when_it_has_a_database() {
        // 指到一个没有 corpus.db 的目录时应当忽略它，退到后面的策略
        let tmp = std::env::temp_dir();
        unsafe { std::env::set_var("JPOP_CORPUS_HOME", &tmp) };
        let (root, _source) = library_root::locate();
        unsafe { std::env::remove_var("JPOP_CORPUS_HOME") };
        assert_ne!(root, tmp, "指到没有 corpus.db 的目录时不该采纳");
    }
}
