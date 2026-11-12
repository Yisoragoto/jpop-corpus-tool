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
//! `tone-alac.m4a`、`tone.aif(f)`、`tone-adpcm.wav` 是 `make_lossless.py` 从 `tone.wav`
//! 的样本手写出来的（ALAC 的帧不压缩，只证明认得出、解得开）。测试本身不需要 ffmpeg。
//!
//! 同一个扩展名里装着不同编码的，单独配夹具：`.m4a` 有 AAC 和 ALAC，`.wav` 有 PCM 和 ADPCM。

use std::path::PathBuf;

use jp_import::scan::{AUDIO_EXTS, UNSUPPORTED_AUDIO};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/formats").join(name)
}

fn fixture(ext: &str) -> PathBuf {
    let path = fixture_path(&format!("tone.{ext}"));
    assert!(path.is_file(), "没有 {ext} 的夹具（{}）。新加了格式的话要配一个", path.display());
    path
}

/// 扩展名一样、编码不一样的那几个夹具
const VARIANTS: &[&str] = &["tone-alac.m4a", "tone-adpcm.wav"];

/// 不支持的格式里，必须有真文件证明「解不开」的。
///
/// 其余的（APE、WavPack、DSD、各家客户端加密的……）没有夹具：生成要用这里不带的编码器，
/// 加密格式还得拿真歌去做。它们的扩展名本来就不在解码器的识别范围里。
const UNSUPPORTED_WITH_FIXTURE: &[&str] = &["opus", "wma"];

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
    for ext in UNSUPPORTED_WITH_FIXTURE {
        assert!(UNSUPPORTED_AUDIO.iter().any(|(e, _)| e == ext), "{ext} 不在 UNSUPPORTED_AUDIO 里了");
    }
    for (ext, reason) in UNSUPPORTED_AUDIO {
        let path = fixture_path(&format!("tone.{ext}"));
        if !UNSUPPORTED_WITH_FIXTURE.contains(ext) && !path.is_file() {
            continue;
        }
        let path = fixture(ext);
        assert!(
            jp_audio::decode_all(&path).is_err(),
            "{ext} 现在解得开了：把它从 UNSUPPORTED_AUDIO 挪到 AUDIO_EXTS（原因那句「{reason}」也不成立了）"
        );
    }
}

/// 无损格式：解出来的每一个样本都要和同一段音频的 WAV 一样，位置也一样。
/// 曲库里几乎全是 FLAC，Anki 片段的起止时间靠的就是「第 N 个样本就是第 N 个样本」。
#[test]
fn lossless_formats_decode_to_exactly_the_samples_of_the_wav() {
    let wav = jp_audio::decode_all(&fixture("wav")).unwrap();
    for name in ["tone.flac", "tone-alac.m4a", "tone.aiff", "tone.aif"] {
        let pcm = jp_audio::decode_all(&fixture_path(name)).unwrap_or_else(|err| panic!("{name} 解不开：{err:#}"));
        assert_eq!(pcm.frames(), 22_050, "{name}");
        assert!(pcm == wav, "{name} 和 WAV 解出来的样本不一样");
    }
}

/// 同一个扩展名里的另一种编码也得解得开：ALAC 的 .m4a 以前能导进库、放不出声。
#[test]
fn other_codecs_behind_a_known_extension_decode() {
    for name in VARIANTS {
        let path = fixture_path(name);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap();
        assert!(AUDIO_EXTS.contains(&ext), "{name}");
        let pcm = jp_audio::decode_all(&path).unwrap_or_else(|err| panic!("{name} 解不开：{err:#}"));
        assert_eq!((pcm.channels, pcm.rate), (1, 22_050), "{name}");
        // ADPCM 按块编码，最后一块补齐，会多出不到一块（1017 个样本）
        assert!((pcm.seconds() - 1.0).abs() < 0.05, "{name}：解出来 {} 秒", pcm.seconds());
        let peak = pcm.samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!(peak > 0.05, "{name}：解出来是静音（峰值 {peak}）");
        assert!(jp_audio::probe_duration(&path).is_ok(), "{name}");
    }
}

/// 每种格式：按时间段解出来的，和整首解出来再截取的，是同一批样本。
#[test]
fn a_window_of_every_format_is_a_slice_of_the_full_decode() {
    let paths = AUDIO_EXTS.iter().map(|ext| fixture(ext)).chain(VARIANTS.iter().map(|name| fixture_path(name)));
    for path in paths {
        let ext = path.file_name().and_then(|n| n.to_str()).unwrap().to_owned();
        let whole = jp_audio::decode_all(&path).unwrap();
        let piece = jp_audio::decode_window(&path, 0.25, 0.75).unwrap();
        let (a, b) = ((0.25f64 * 22_050.0).round() as usize, (0.75f64 * 22_050.0).round() as usize);
        assert!(piece.samples == whole.samples[a..b], "{ext}：时间段和整首对不上");
    }
}

/// 内容身份（`jp_corpus::fingerprint`）用的是另一份 rodio 依赖，格式得和播放开的一样：
/// 播放解得开、指纹解不开，这首歌就没有身份，改名挪位置后认不回来。
/// 指纹只看 PCM，同一段音频的几种无损格式要算出同一个指纹。
#[test]
fn lossless_formats_share_one_content_fingerprint() {
    let wav = jp_corpus::fingerprint::of(&fixture("wav")).unwrap();
    for name in ["tone.flac", "tone-alac.m4a", "tone.aiff", "tone.aif"] {
        let print = jp_corpus::fingerprint::of(&fixture_path(name)).unwrap_or_else(|err| panic!("{name} 算不出指纹：{err:#}"));
        assert_eq!(print, wav, "{name}");
    }
    for ext in AUDIO_EXTS {
        jp_corpus::fingerprint::of(&fixture(ext)).unwrap_or_else(|err| panic!("{ext} 算不出指纹：{err:#}"));
    }
    for name in VARIANTS {
        jp_corpus::fingerprint::of(&fixture_path(name)).unwrap_or_else(|err| panic!("{name} 算不出指纹：{err:#}"));
    }
}
