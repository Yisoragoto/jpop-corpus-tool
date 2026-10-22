//! 前 30 秒 → 8 kHz 单声道有符号 PCM16（小端）→ SHA-256。
//!
//! 这是同文件身份：改名、移动后仍认得出。不是跨压制识别：不同压制可能不同，
//! 不同音频也可能共享这 30 秒。跨压制需要 Chromaprint，本期不做，不拿它自动合并歌曲。
use std::{fs::File, path::Path};
use anyhow::{Context, Result, ensure};
use rodio::{Decoder, Source};
use rusqlite::{Connection, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const ALGORITHM: &str = "pcm8k-mono-i16le-30s-v1";

pub fn of(path: &Path) -> Result<String> {
    let mut decoder = Decoder::try_from(File::open(path)?).with_context(|| format!("无法解码音频指纹：{}", path.display()))?;
    let rate = decoder.sample_rate().get() as usize;
    let channels = usize::from(decoder.channels().get());
    ensure!(rate > 0 && channels > 0, "音频指纹的采样率或声道无效");
    let mut mono = Vec::new();
    for _ in 0..rate * 30 {
        let mut sum = 0.0_f64;
        let mut complete = true;
        for _ in 0..channels {
            match decoder.next() { Some(v) => sum += f64::from(v), None => { complete = false; break; } }
        }
        if !complete { break; }
        ensure!(sum.is_finite(), "音频指纹遇到无效 PCM 样本");
        mono.push(sum / channels as f64);
    }
    ensure!(!mono.is_empty(), "音频没有可用于指纹的样本");
    let count = (mono.len() * 8000).div_ceil(rate).min(30 * 8000);
    let mut hash = Sha256::new();
    for i in 0..count {
        let position = i as f64 * rate as f64 / 8000.0;
        let left = position as usize;
        let right = (left + 1).min(mono.len() - 1);
        let value = mono[left] + (mono[right] - mono[left]) * position.fract();
        let pcm = (value.clamp(-1.0, 1.0) * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
        hash.update(pcm.to_le_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn refresh(conn: &Connection, song_id: &str, path: &Path) -> Result<()> {
    let fingerprint = of(path)?;
    crate::identity::atomic(conn, || {
    ensure!(conn.execute("UPDATE songs SET audio_fingerprint=?1 WHERE id=?2", params![fingerprint, song_id])? == 1, "指纹对应的歌曲不存在");
    crate::identity::external_id(conn, "song", song_id, ALGORITHM, &fingerprint, "audio", Some(1.0))?;
    crate::lineage::set_provenance(conn, "song", song_id, "audio_fingerprint", "audio", &format!("{ALGORITHM}:{}", path.display()), Some(1.0))?;
    Ok(())
    })
}

#[derive(Debug, Default, Serialize)]
pub struct Rescanned { pub updated: usize, pub errors: Vec<String> }

pub fn rescan(corpus: &crate::Corpus) -> Result<Rescanned> {
    let mut report = Rescanned::default();
    for track in corpus.tracks(i64::MAX)? {
        match refresh(corpus.connection(), &track.id, Path::new(&track.audio_path)) {
            Ok(()) => report.updated += 1,
            Err(err) => report.errors.push(format!("{}: {err:#}", track.id)),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wav(path: &Path, samples: &[i16], rate: u32) {
        let mut data = b"RIFF".to_vec();
        data.extend_from_slice(&(36 + samples.len() as u32 * 2).to_le_bytes());
        data.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
        data.extend_from_slice(&rate.to_le_bytes()); data.extend_from_slice(&(rate * 2).to_le_bytes());
        data.extend_from_slice(b"\x02\0\x10\0data"); data.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
        for value in samples { data.extend_from_slice(&value.to_le_bytes()); }
        std::fs::write(path, data).unwrap();
    }
    #[test]
    fn rename_keeps_identity_and_different_audio_changes_it() {
        let dir = std::env::temp_dir().join(format!("jp-fingerprint-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("original.wav"); let moved = dir.join("renamed.wav"); let other = dir.join("other.wav");
        wav(&first, &[100; 8000], 8000); let fingerprint = of(&first).unwrap();
        std::fs::rename(&first, &moved).unwrap();
        assert_eq!(fingerprint, of(&moved).unwrap());
        wav(&other, &[-100; 8000], 8000); assert_ne!(fingerprint, of(&other).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn only_the_first_thirty_seconds_are_used_and_resampling_is_deterministic() {
        let dir = std::env::temp_dir().join(format!("jp-fingerprint-window-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("a.wav"); let second = dir.join("b.wav"); let fast = dir.join("c.wav");
        let samples = vec![123; 8000 * 31]; wav(&first, &samples, 8000);
        let mut changed = samples.clone(); changed[8000 * 30..].fill(456); wav(&second, &changed, 8000);
        wav(&fast, &vec![123; 16000 * 31], 16000);
        assert_eq!(of(&first).unwrap(), of(&second).unwrap());
        assert_eq!(of(&first).unwrap(), of(&fast).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }
}
