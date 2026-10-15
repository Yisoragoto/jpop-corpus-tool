//! 数一数一批音频里有多少内嵌了歌词、封面，以及导入时选歌词的结果和只认外挂 .lrc 时差几首：
//! `cargo run -p jp-import --example embedded_survey -- <目录>...`
use std::path::Path;

use jp_import::LyricsPick;

fn main() {
    for dir in std::env::args().skip(1) {
        let tracks = jp_import::scan_dir(Path::new(&dir), 6);
        let (mut synced, mut plain, mut covers, mut changed) = (0, 0, 0, 0);
        let mut sizes = Vec::new();
        for track in &tracks {
            match &track.embedded_lyrics {
                Some(l) if l.synced => synced += 1,
                Some(_) => plain += 1,
                None => {}
            }
            // 只认外挂 .lrc 的旧行为
            let old = track.lyrics_path.as_deref().map(LyricsPick::File);
            if track.lyrics_pick() != old {
                changed += 1;
            }
            if let Some(cover) = jp_import::embedded::read_cover(Path::new(&track.path)) {
                covers += 1;
                sizes.push(cover.data.len());
            }
        }
        sizes.sort_unstable();
        println!(
            "{dir}: {} 个文件，内嵌歌词 {}（带时间轴 {synced}、纯文本 {plain}），\
             歌词来源和旧版不同 {changed} 首；内嵌封面 {covers}，大小 {:?}..{:?} 字节",
            tracks.len(),
            synced + plain,
            sizes.first(),
            sizes.last()
        );
    }
}
