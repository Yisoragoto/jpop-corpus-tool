//! 导入认的格式，和播放器解得开的格式，必须是同一批。
//!
//! 这两张表在两个 crate 里（`jp_import::scan` 和 `jp-audio` 的解码器），以前对不上：
//! `AUDIO_EXTS` 里有 opus 和 wma，解码器却没有这两种——能导进库、放不出声。
//! 这里拿每种格式的一个真文件去解，把两张表钉在一起：
//!
//! - `AUDIO_EXTS` 里的每一种都要解得开；
//! - `UNSUPPORTED_AUDIO` 里的每一种都要解不开。哪天加了 Opus 解码器，这条会红，
//!   提醒把它从「不支持」挪回「能导入」。
//!
//! 夹具在 `tests/data/formats/`，每个是 1 秒、22050 Hz、单声道的 440 Hz 正弦，
//! 用 ffmpeg 8.1 生成一次后提交（`-f lavfi -i sine=frequency=440:duration=1:sample_rate=22050`）。
//! 测试本身不需要 ffmpeg。

use std::path::PathBuf;

use jp_import::scan::{AUDIO_EXTS, UNSUPPORTED_AUDIO};

fn fixture(ext: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/formats").join(format!("tone.{ext}"));
    assert!(path.is_file(), "没有 {ext} 的夹具（{}）。新加了格式的话要配一个", path.display());
    path
}

#[test]
fn every_importable_format_decodes_and_every_unsupported_one_does_not() {
    for ext in AUDIO_EXTS {
        let path = fixture(ext);
        let pcm = jp_audio::decode_all(&path).unwrap_or_else(|err| panic!("{ext} 在 AUDIO_EXTS 里，却解不开：{err:#}"));
        assert_eq!((pcm.channels, pcm.rate), (1, 22_050), "{ext}");
        // 有损格式头尾会多出或少掉一点编码器的填充，1 秒上下就行
        assert!((pcm.seconds() - 1.0).abs() < 0.12, "{ext}：解出来 {} 秒", pcm.seconds());
        let peak = pcm.samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!(peak > 0.05, "{ext}：解出来是静音（峰值 {peak}）");
        // 时长探测走的是同一个解码器，也得认
        assert!(jp_audio::probe_duration(&path).is_ok(), "{ext}");
    }
    for (ext, reason) in UNSUPPORTED_AUDIO {
        let path = fixture(ext);
        assert!(
            jp_audio::decode_all(&path).is_err(),
            "{ext} 现在解得开了：把它从 UNSUPPORTED_AUDIO 挪到 AUDIO_EXTS（原因那句「{reason}」也不成立了）"
        );
    }
}

/// FLAC 是无损的：解出来的每一个样本都要和同一段音频的 WAV 一样，位置也一样。
/// 曲库里几乎全是 FLAC，Anki 片段的起止时间靠的就是「第 N 个样本就是第 N 个样本」。
#[test]
fn flac_decodes_to_exactly_the_samples_of_the_wav() {
    let flac = jp_audio::decode_all(&fixture("flac")).unwrap();
    let wav = jp_audio::decode_all(&fixture("wav")).unwrap();
    assert_eq!(flac.frames(), 22_050);
    assert!(flac == wav, "FLAC 和 WAV 解出来的样本不一样");
}

/// 每种格式：按时间段解出来的，和整首解出来再截取的，是同一批样本。
#[test]
fn a_window_of_every_format_is_a_slice_of_the_full_decode() {
    for ext in AUDIO_EXTS {
        let path = fixture(ext);
        let whole = jp_audio::decode_all(&path).unwrap();
        let piece = jp_audio::decode_window(&path, 0.25, 0.75).unwrap();
        let (a, b) = ((0.25f64 * 22_050.0).round() as usize, (0.75f64 * 22_050.0).round() as usize);
        assert!(piece.samples == whole.samples[a..b], "{ext}：时间段和整首对不上");
    }
}
