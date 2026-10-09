//! `metadata/songs.csv`：Python 版一直和 `songs` 表同步维护的曲目清单（`id,title,artist,year,album,genre,audio_path`）。
//!
//! Python 版加歌、改歌、删歌、修复音频路径时都会改它（`legacy/gui.py` 的 `AddSongWorker`、`RepairAudioDialog`，
//! `song_manager._edit` / `_delete`），`legacy/scripts/03_build_db.py` 重建库时以它为准。这里照同样的时机、同样的粒度改：
//! **只改动到的那几行**，改动的值取数据库里的当前值；其余行一个字节不动。
//!
//! 格式和 Python `csv` 模块（`utf-8-sig`、`DictWriter`、默认 `excel` 方言）逐字节一致：开头 BOM，行尾 CRLF，
//! 字段里有逗号、双引号、换行才加引号，双引号写两个。读写往返用真实文件对过（见测试）。
//! 文件不存在就不建——没有这份清单的用户不需要它。写入先写临时文件再改名，写到一半断电不会留下半截清单。

use std::path::Path;

use anyhow::{Context, Result, bail};

pub const HEADER: [&str; 7] = ["id", "title", "artist", "year", "album", "genre", "audio_path"];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CsvSong {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    pub album: String,
    pub genre: String,
    pub audio_path: String,
}

impl CsvSong {
    fn fields(&self) -> [&str; 7] {
        [&self.id, &self.title, &self.artist, &self.year, &self.album, &self.genre, &self.audio_path]
    }
}

/// 按 Python `csv.reader`（excel 方言）的规则拆行：引号里的逗号、换行不算分隔，`""` 是一个引号
fn parse_records(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut pending = false;
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' => {
                quoted = true;
                pending = true;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                pending = true;
            }
            '\r' | '\n' => {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if pending || !field.is_empty() || !record.is_empty() {
                    record.push(std::mem::take(&mut field));
                    records.push(std::mem::take(&mut record));
                }
                pending = false;
            }
            _ => {
                field.push(ch);
                pending = true;
            }
        }
    }
    if pending || !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

/// Python `csv.writer` 默认 QUOTE_MINIMAL：有分隔符、引号、换行才加引号
fn write_field(out: &mut String, field: &str) {
    if field.contains([',', '"', '\r', '\n']) {
        out.push('"');
        out.push_str(&field.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(field);
    }
}

pub fn serialize(rows: &[CsvSong]) -> Vec<u8> {
    let mut out = String::from("\u{FEFF}");
    out.push_str(&HEADER.join(","));
    out.push_str("\r\n");
    for row in rows {
        for (i, field) in row.fields().iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write_field(&mut out, field);
        }
        out.push_str("\r\n");
    }
    out.into_bytes()
}

pub fn parse(bytes: &[u8]) -> Result<Vec<CsvSong>> {
    let text = std::str::from_utf8(bytes).context("songs.csv 不是 UTF-8")?;
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut records = parse_records(text).into_iter();
    let header = records.next().unwrap_or_default();
    let index = |name: &str| header.iter().position(|h| h == name);
    let columns: Vec<Option<usize>> = HEADER.iter().map(|h| index(h)).collect();
    if columns[0].is_none() {
        bail!("songs.csv 没有 id 列");
    }
    let get = |record: &[String], col: Option<usize>| col.and_then(|c| record.get(c)).cloned().unwrap_or_default();
    Ok(records
        .map(|r| CsvSong {
            id: get(&r, columns[0]),
            title: get(&r, columns[1]),
            artist: get(&r, columns[2]),
            year: get(&r, columns[3]),
            album: get(&r, columns[4]),
            genre: get(&r, columns[5]),
            audio_path: get(&r, columns[6]),
        })
        .collect())
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("写不了 {}", Path::new(&tmp).display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("替换不了 {}", path.display()))?;
    Ok(())
}

/// 有这几首就改那几行，没有就追加到末尾。清单文件不存在时什么都不做，返回 false
pub fn upsert(path: &Path, songs: &[CsvSong]) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let mut rows = parse(&std::fs::read(path)?)?;
    for song in songs {
        match rows.iter_mut().find(|r| r.id == song.id) {
            Some(row) => *row = song.clone(),
            None => rows.push(song.clone()),
        }
    }
    write_atomically(path, &serialize(&rows))?;
    Ok(true)
}

/// 删掉这几首。清单文件不存在时什么都不做，返回 false
pub fn remove(path: &Path, ids: &[&str]) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let mut rows = parse(&std::fs::read(path)?)?;
    rows.retain(|r| !ids.contains(&r.id.as_str()));
    write_atomically(path, &serialize(&rows))?;
    Ok(true)
}

/// 从数据库读一首歌在清单里该写的样子
pub fn from_db(conn: &rusqlite::Connection, song_id: &str) -> Result<Option<CsvSong>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT id, title, artist, COALESCE(year,''), COALESCE(album,''), COALESCE(genre,''), COALESCE(audio_path,'') \
             FROM songs WHERE id=?1",
            rusqlite::params![song_id],
            |r| {
                Ok(CsvSong {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    year: r.get(3)?,
                    album: r.get(4)?,
                    genre: r.get(5)?,
                    audio_path: r.get(6)?,
                })
            },
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_follows_python_csv_minimal() {
        let row = CsvSong {
            id: "104".into(),
            title: "春愁".into(),
            album: "Love Me, Love You".into(),
            genre: "say \"hi\"".into(),
            artist: "two\nlines".into(),
            ..Default::default()
        };
        let bytes = serialize(std::slice::from_ref(&row));
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.starts_with("\u{FEFF}id,title,artist,year,album,genre,audio_path\r\n"));
        assert!(text.ends_with("104,春愁,\"two\nlines\",,\"Love Me, Love You\",\"say \"\"hi\"\"\",\r\n"), "{text:?}");
        assert_eq!(parse(&bytes).unwrap(), vec![row]);
    }

    #[test]
    fn upsert_changes_only_the_named_rows_and_remove_drops_them() {
        let dir = std::env::temp_dir().join(format!("jp-import-csv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("songs.csv");
        let song = |id: &str, title: &str| CsvSong { id: id.into(), title: title.into(), artist: "A".into(), ..Default::default() };
        std::fs::write(&path, serialize(&[song("001", "一"), song("002", "二")])).unwrap();

        assert!(upsert(&path, &[song("002", "二改"), song("003", "三")]).unwrap());
        let rows = parse(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["一", "二改", "三"]);

        assert!(remove(&path, &["001"]).unwrap());
        let rows = parse(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["002", "003"]);
        assert!(!Path::new(&format!("{}.tmp", path.display())).exists());

        assert!(!upsert(&dir.join("没有这个.csv"), &[song("009", "九")]).unwrap(), "清单不存在时不建");
        assert!(!dir.join("没有这个.csv").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 真实清单读进来再原样写出去，逐字节相同（没有这个文件就跳过）
    #[test]
    fn the_real_songs_csv_round_trips_byte_for_byte() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../metadata/songs.csv");
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("跳过：没有 metadata/songs.csv");
            return;
        };
        let rows = parse(&bytes).unwrap();
        assert!(!rows.is_empty());
        assert!(serialize(&rows) == bytes, "往返后不一致");
    }
}
