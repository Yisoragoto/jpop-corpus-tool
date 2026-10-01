//! 释义块取多大：把词典库里已导入的释义按导入顺序还原出来，按不同块大小重新分块压缩，
//! 量体积和「解一块」的用时。查词基准显示 91% 的时间花在解压释义上，这里决定怎么改。
//!
//! ```text
//! cargo run -p jp-dict --release --example block_size_study -- <dictionaries.db> [词典名...]
//! ```

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use rusqlite::{Connection, OpenFlags};

/// 每本最多取这么多原始释义来试（按导入顺序的前缀），免得大辞泉跑几分钟
const SAMPLE_BYTES: usize = 80_000_000;
/// 查词基准里一次查词平均要解的释义条数
const GLOSSARIES_PER_LOOKUP: u32 = 117;

fn compress(buf: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(buf)?;
    Ok(encoder.finish()?)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let db = args.next().context("用法: block_size_study <dictionaries.db> [词典名...]")?;
    let mut titles: Vec<String> = args.collect();
    if titles.is_empty() {
        titles = ["大辞泉 第二版", "明鏡国語辞典 第三版", "Jitendex.org [2026-01-04]", "明鏡日汉双解辞典", "JMnedict"]
            .into_iter()
            .map(String::from)
            .collect();
    }
    let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    for title in &titles {
        let Ok(id) = conn.query_row("SELECT id FROM dictionaries WHERE title = ?1", [title], |r| r.get::<_, i64>(0)) else {
            println!("\n{title}: 库里没有");
            continue;
        };
        let mut entries: Vec<Vec<u8>> = Vec::new();
        let mut raw_total = 0usize;
        let mut cached: (i64, Vec<u8>) = (-1, Vec::new());
        let mut stmt = conn.prepare("SELECT block_id, block_offset, block_len FROM terms WHERE dictionary_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as usize, r.get::<_, i64>(2)? as usize)))?;
        for row in rows {
            let (block, offset, len) = row?;
            if cached.0 != block {
                let data: Vec<u8> = conn.query_row("SELECT data FROM glossary_blocks WHERE id = ?1", [block], |r| r.get(0))?;
                let mut raw = Vec::new();
                DeflateDecoder::new(&data[..]).read_to_end(&mut raw)?;
                cached = (block, raw);
            }
            entries.push(cached.1[offset..offset + len].to_vec());
            raw_total += len;
            if raw_total >= SAMPLE_BYTES {
                break;
            }
        }
        println!("\n{title}：取前 {} 条，释义 {:.1} MB，平均每条 {} B", entries.len(), raw_total as f64 / 1e6, raw_total / entries.len().max(1));

        for target in [1024usize, 4096, 8192, 16384, 32768, 65536] {
            let mut blocks: Vec<Vec<u8>> = Vec::new();
            let mut buf = Vec::with_capacity(target * 2);
            for entry in &entries {
                buf.extend_from_slice(entry);
                if buf.len() >= target {
                    blocks.push(compress(&buf)?);
                    buf.clear();
                }
            }
            if !buf.is_empty() {
                blocks.push(compress(&buf)?);
            }
            let compressed: usize = blocks.iter().map(Vec::len).sum();

            // 均匀抽 3000 块，各解一次
            let step = (blocks.len() / 3000).max(1);
            let sample: Vec<&Vec<u8>> = blocks.iter().step_by(step).collect();
            let mut out = Vec::with_capacity(target * 2);
            let started = Instant::now();
            for block in &sample {
                out.clear();
                DeflateDecoder::new(&block[..]).read_to_end(&mut out)?;
            }
            let per_block: Duration = started.elapsed() / sample.len().max(1) as u32;
            println!(
                "  块 {:>5} B：{:>7} 块  压缩后 {:>6.1} MB（{:>4.1}%）  解一块 {:>8.1?}  一次查词上限约 {:>7.1?}",
                target,
                blocks.len(),
                compressed as f64 / 1e6,
                compressed as f64 * 100.0 / raw_total.max(1) as f64,
                per_block,
                per_block * GLOSSARIES_PER_LOOKUP
            );
        }
    }
    Ok(())
}
