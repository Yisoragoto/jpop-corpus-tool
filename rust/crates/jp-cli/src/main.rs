use std::{io::Write, path::{Path, PathBuf}, process::ExitCode};

use anyhow::{Context, Result, bail, ensure};
use jp_corpus::{Corpus, library_root};

const USAGE: &str = "用法：jp info | scan <path> [--json] | import <path> [--tokenize] [--json] | tokenize [--missing] | scrape <path...> | export-anki --out <file.tsv>";

#[derive(Debug, PartialEq)]
enum Command {
    Info,
    Scan { path: PathBuf, json: bool },
    Import { path: PathBuf, tokenize: bool, json: bool },
    Tokenize { missing: bool },
    Scrape { paths: Vec<String> },
    Export { out: PathBuf },
}

fn flag(args: &mut Vec<String>, flag: &str) -> bool {
    if let Some(i) = args.iter().position(|a| a == flag) { args.remove(i); true } else { false }
}

fn path(args: &[String]) -> Result<PathBuf> {
    ensure!(args.len() == 1 && !args[0].starts_with('-'), "{USAGE}");
    Ok(PathBuf::from(&args[0]))
}

fn parse(mut args: Vec<String>) -> Result<Command> {
    ensure!(!args.is_empty(), "{USAGE}");
    let name = args.remove(0);
    Ok(match name.as_str() {
        "info" if args.is_empty() => Command::Info,
        "scan" => { let json = flag(&mut args, "--json"); Command::Scan { path: path(&args)?, json } }
        "import" => {
            let json = flag(&mut args, "--json");
            let tokenize = flag(&mut args, "--tokenize");
            Command::Import { path: path(&args)?, tokenize, json }
        }
        "tokenize" => {
            let missing = flag(&mut args, "--missing");
            ensure!(args.is_empty(), "{USAGE}");
            Command::Tokenize { missing }
        }
        "scrape" if !args.is_empty() && args.iter().all(|a| !a.starts_with('-')) => Command::Scrape { paths: args },
        "export-anki" if args.first().map(String::as_str) == Some("--out") => Command::Export { out: path(&args[1..])? },
        _ => bail!("{USAGE}"),
    })
}

fn analyzer(root: &Path) -> Result<jp_tokenizer::Analyzer> {
    let (resources, dict) = jp_tokenizer::locate_sudachipy(root).context("找不到 Sudachi 词典，请先在应用里安装分词词典")?;
    jp_tokenizer::Analyzer::from_sudachipy(&resources, &dict)
}

fn open(root: &Path) -> Result<Corpus> {
    std::fs::create_dir_all(root)?;
    let corpus = Corpus::open_writable(root.join("corpus.db"))?;
    corpus.ensure_schema()?;
    Ok(corpus)
}

fn scanned(path: &Path) -> Result<Vec<jp_import::ScannedTrack>> {
    ensure!(path.exists(), "路径不存在：{}", path.display());
    let (tracks, _) = jp_import::scan_targets(&[path.to_string_lossy().into_owned()], 12);
    Ok(tracks)
}

fn execute(command: Command) -> Result<()> {
    let (root, _) = library_root::locate();
    match command {
        Command::Info => {
            let corpus = open(&root)?;
            let stats = corpus.overview()?;
            println!("库：{}\n结构版本：{}\n歌曲：{}，歌词行：{}，词次：{}", root.display(), jp_corpus::migrations::version(corpus.connection())?, stats.tracks, stats.lyric_lines, stats.tokens);
        }
        Command::Scan { path, json } => {
            let tracks = scanned(&path)?;
            // 扫描空库也不能建 corpus.db；已有库按只读连接查询，避免“预览”升级用户数据。
            let index = if library_root::looks_like_library(&root) {
                jp_import::LibraryIndex::from_corpus(&Corpus::open(root.join("corpus.db"))?)?
            } else { jp_import::LibraryIndex::from_tracks(&[]) };
            let result = jp_import::review(&tracks, &index, jp_tokenizer::locate_sudachipy(&root).is_some(), vec![]);
            if json { println!("{}", serde_json::to_string(&result)?); }
            else { println!("{} 个音频，新歌 {} 首", result.summary.total, result.summary.new); }
        }
        Command::Import { path, tokenize, json } => {
            let tracks = scanned(&path)?;
            let tokenizer = if tokenize { Some(analyzer(&root)?) } else { None };
            let mut corpus = open(&root)?;
            let plan = jp_import::plan(&tracks, &jp_import::LibraryIndex::from_corpus(&corpus)?);
            let report = jp_import::execute_with_progress(corpus.connection_mut(), &plan, tokenizer.as_ref(), |track, done, total| eprintln!("{done}/{total} {}", track.title))?;
            if json { println!("{}", serde_json::to_string(&report)?); }
            else { println!("导入 {} 首，失败 {} 首", report.imported(), report.failed()); }
            ensure!(report.failed() == 0, "部分歌曲导入失败，请查看报告");
        }
        Command::Tokenize { missing } => {
            let tokenizer = analyzer(&root)?;
            let mut corpus = open(&root)?;
            let tx = corpus.connection_mut().transaction()?;
            if missing {
                let result = jp_import::maintain::tokenize_missing(&tx, &tokenizer, |song| eprintln!("分词：{}", song.song_id))?;
                println!("分词 {} 首，{} 词次", result.songs, result.tokens);
            } else {
                // 明确不带 --missing 才重分全库，恢复校正仍交给现有导入核心。
                let ids = Corpus::open(root.join("corpus.db"))?.tracks(i64::MAX)?;
                for track in ids {
                    let result = jp_import::maintain::tokenize_song(&tx, &track.id, &tokenizer)?;
                    eprintln!("分词：{}，{} 词次", track.title, result.tokens);
                }
            }
            tx.commit()?;
        }
        Command::Scrape { paths } => scrape(&root, &paths)?,
        Command::Export { out } => export_anki(&root, &out)?,
    }
    Ok(())
}

fn scrape(root: &Path, paths: &[String]) -> Result<()> {
    for path in paths { ensure!(Path::new(path).exists(), "路径不存在：{path}"); }
    let (tracks, _) = jp_import::scan_targets(paths, 12);
    let corpus = open(root)?;
    let index = jp_import::LibraryIndex::from_corpus(&corpus)?;
    let resolver = jp_scraper::default_resolver(false)?;
    let store = jp_scraper::ScrapeStore::new(corpus.connection());
    store.ensure_schema()?;
    let mut failed = false;
    for track in tracks {
        let input = jp_scraper::TrackFile {
            path: track.path.clone(), filename: track.file_name.clone(),
            embedded_title: track.tag_title.clone(), embedded_artist: track.tag_artist.clone(),
            embedded_album: track.tag_album.clone(), embedded_albumartist: track.tag_album_artist.clone(),
            embedded_year: track.tag_year.clone(), embedded_genre: track.tag_genre.clone(),
            filename_title: track.guess_title.clone(), filename_artist: track.guess_artist.clone(),
            folder_artist: track.folder_artist.clone(), folder_album: track.folder_album.clone(),
            duration_sec: track.duration_sec, track_number: track.track_number.map(i64::from),
            disc_number: track.disc_number.map(i64::from), ..Default::default()
        };
        let song_id = index.song_for_path(&track.path).unwrap_or("");
        store.register(&input, song_id)?;
        let result = resolver.resolve(&input);
        store.record(&result, song_id)?;
        if result.ok() && !song_id.is_empty() && let Some(candidate) = &result.candidate {
            jp_corpus::enrichment::fill_missing(corpus.connection(), song_id, &jp_corpus::enrichment::Metadata {
                album: &candidate.album, year: &candidate.year, genre: &candidate.genre,
                release_date: &candidate.release_date, provider: &candidate.provider,
                provider_id: &candidate.provider_id, confidence: Some(result.confidence),
            })?;
        }
        failed |= result.status == jp_scraper::ScrapeStatus::Failed;
        eprintln!("刮削：{} {}", track.title(), result.status.as_str());
        println!("{}", serde_json::to_string(&result)?);
    }
    ensure!(!failed, "部分歌曲刮削失败，请查看报告");
    Ok(())
}

fn export_anki(root: &Path, out: &Path) -> Result<()> {
    let corpus = open(root)?;
    let words = jp_anki::pick_words(corpus.connection(), &jp_anki::PickOptions { limit: i64::MAX, ..Default::default() }, None)?;
    let mut file = std::io::BufWriter::new(std::fs::File::create(out)?);
    writeln!(file, "#separator:tab\n#html:true\n#notetype:{}\n#columns:{}", jp_anki::NOTE_TYPE, jp_anki::FIELDS.join("\t"))?;
    for word in &words {
        let card = jp_anki::card::build(corpus.connection(), &word.lemma, &word.pos, &[], jp_anki::CardOptions::default())?;
        let fields = card.fields();
        let row: Vec<String> = jp_anki::FIELDS.iter().map(|name| {
            // Anki 的 TSV 引号规则和 CSV 相同；换行留在引号内，释义 HTML 不受损。
            format!("\"{}\"", fields.get(*name).map(String::as_str).unwrap_or("").replace('"', "\"\""))
        }).collect();
        writeln!(file, "{}", row.join("\t"))?;
    }
    file.flush()?;
    eprintln!("导出 {} 张卡片：{}", words.len(), out.display());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--help"] || args == ["-h"] { println!("{USAGE}"); return ExitCode::SUCCESS; }
    let command = match parse(args) { Ok(c) => c, Err(e) => { eprintln!("{e}"); return ExitCode::from(1); } };
    match execute(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => { eprintln!("执行失败：{}", format!("{err:#}").replace(['\r', '\n'], " ")); ExitCode::from(2) }
    }
}
