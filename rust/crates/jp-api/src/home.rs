//! 首页。

use anyhow::Result;
use jp_corpus::home::{AddedTrack, ContinueTrack, DayListening, Reach};
use jp_corpus::{Corpus, Overview, PlayedTrack, Track};
use serde::{Deserialize, Serialize};

use crate::Command;

/// 首页「最近添加」不传条数时摆几首。macOS 一直是 8；Windows 首页只摆 5，由 `jp-app` 显式传
pub const ADDED_LIMIT_DEFAULT: i64 = 8;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct HomeSummaryArgs {
    /// 本地相对 UTC 的分钟数（东八区 +480），「今天」按它算。不传按 UTC
    pub utc_offset_minutes: Option<i32>,
    /// 「最近添加」要几首
    pub added_limit: Option<i64>,
}

/// 首页要的一切。
///
/// 打成一个命令而不是让前端发六次：首页一进来就要全部数据，
/// 分开发不仅多五次往返，还可能拿到互相不一致的快照
/// （比如统计已经更新、最近播放还是旧的）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HomeSummary {
    pub overview: Overview,
    /// 上次听的那首和停在哪；没有收听记录时是 null
    pub continue_track: Option<ContinueTrack>,
    pub recently_played: Vec<PlayedTrack>,
    pub favorites: Vec<Track>,
    pub today: DayListening,
    pub reach: Reach,
    pub recently_added: Vec<AddedTrack>,
}

pub struct GetHomeSummary;

impl Command for GetHomeSummary {
    const TAURI: &'static str = "home_summary";
    const FFI: &'static str = "homeSummary";
    type Args = HomeSummaryArgs;
    type Output = HomeSummary;
    fn run(corpus: &Corpus, args: HomeSummaryArgs) -> Result<HomeSummary> {
        Ok(HomeSummary {
            overview: corpus.overview()?,
            continue_track: corpus.continue_track()?,
            recently_played: corpus.recently_played(12)?,
            favorites: corpus.favorite_tracks(12)?,
            today: corpus.today_listening(args.utc_offset_minutes.unwrap_or(0))?,
            reach: corpus.reach()?,
            recently_added: corpus.recently_added(args.added_limit.unwrap_or(ADDED_LIMIT_DEFAULT))?,
        })
    }
}
