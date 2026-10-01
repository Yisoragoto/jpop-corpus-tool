//! 分词器回归验收。
//!
//! 两级，严格程度不同（见 `docs/tokenizer.md`）：
//!
//! 1. **sudachi.rs vs SudachiPy —— 应该逐 token 一致**（阈值 0.99）。
//!    同一个词典、同一份配置、同一个 SplitMode，差得多就说明配错了。
//!    这是主要验收标准，也是最灵敏的探针。
//! 2. **sudachi.rs + pos_map vs GiNZA** —— 跨分词器比较，阈值取自
//!    SudachiPy 实测水平减去余量。这是「迁移后语料一致性」的度量。
//!
//! 阈值和基准数据由 `python scripts/export_tokenizer_baseline.py` 生成，
//! 测试直接读 `benchmark/baseline.json`，不在 Rust 侧抄一遍。
//!
//! 基准文件不存在时整个测试跳过——克隆仓库的人不该因为缺 14MB 的
//! 夹具而看到一片红。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use jp_tokenizer::{Analyzer, locate_sudachipy, to_upos};
use serde::Deserialize;

#[derive(Deserialize)]
struct Baseline {
    thresholds: Thresholds,
    pos_map: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Thresholds {
    sudachi_rs_vs_sudachi_py_token_identity: f64,
    upos_exact_min: f64,
    content_binary_min: f64,
    lemma_exact_min: f64,
    boundary_identical_content_min: f64,
}

#[derive(Deserialize)]
struct SudachiLine {
    text: String,
    tokens: Vec<SudachiToken>,
}

#[derive(Deserialize)]
struct SudachiToken {
    surface: String,
    lemma: String,
    normalized: String,
    reading: String,
    pos: Vec<String>,
    upos: String,
}

#[derive(Deserialize)]
struct GinzaLine {
    text: String,
    tokens: Vec<GinzaToken>,
}

#[derive(Deserialize)]
struct GinzaToken {
    surface: String,
    lemma: String,
    pos: String,
}

fn project_root() -> PathBuf {
    // crates/jp-tokenizer -> crates -> rust -> 项目根
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("manifest 路径异常")
        .to_path_buf()
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<T> {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("读不到 {}: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("jsonl 解析失败"))
        .collect()
}

/// 缺基准数据时跳过，并说清楚怎么生成。
macro_rules! require {
    ($path:expr) => {
        if !$path.exists() {
            eprintln!(
                "跳过：缺 {}\n  先跑 python scripts/export_tokenizer_baseline.py",
                $path.display()
            );
            return;
        }
    };
}

fn load_baseline(root: &Path) -> Option<Baseline> {
    let path = root.join("benchmark/baseline.json");
    if !path.exists() {
        return None;
    }
    Some(serde_json::from_str(&fs::read_to_string(&path).ok()?).expect("baseline.json 解析失败"))
}

fn analyzer(root: &Path) -> Option<Analyzer> {
    let (resources, dict) = locate_sudachipy(root)?;
    Some(Analyzer::from_sudachipy(&resources, &dict).expect("加载词典失败"))
}

/// 字符区间，用于跨分词器对齐。和 Python 侧 `_spans` 完全相同。
fn char_spans(surfaces: &[String]) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(surfaces.len());
    let mut pos = 0usize;
    for s in surfaces {
        let len = s.chars().count();
        out.push((pos, pos + len));
        pos += len;
    }
    out
}

// ────────────────────────── 表一致性 ──────────────────────────

#[test]
fn pos_map_matches_python() {
    let root = project_root();
    let Some(baseline) = load_baseline(&root) else {
        eprintln!("跳过：缺 benchmark/baseline.json");
        return;
    };

    let mut mismatched = Vec::new();
    for (key, expected) in &baseline.pos_map {
        let parts: Vec<String> = key.split(',').map(str::to_string).collect();
        let got = to_upos(&parts);
        if got != expected {
            mismatched.push(format!("{key}: Rust={got} Python={expected}"));
        }
    }
    assert!(
        mismatched.is_empty(),
        "Rust 与 Python 的 POS 映射不一致（{} 条）：\n  {}",
        mismatched.len(),
        mismatched.join("\n  ")
    );
}

// ────────────────────────── 第一级验收 ──────────────────────────

#[test]
fn tier1_matches_sudachipy_token_for_token() {
    let root = project_root();
    let path = root.join("benchmark/sudachi_reference.jsonl");
    require!(path);
    let Some(baseline) = load_baseline(&root) else { return };
    let Some(analyzer) = analyzer(&root) else {
        eprintln!("跳过：找不到 venv 里的 SudachiPy 资源");
        return;
    };

    let reference: Vec<SudachiLine> = read_jsonl(&path);
    let mut total = 0usize;
    let mut identical = 0usize;
    let mut count_mismatch = 0usize;
    let mut samples: Vec<String> = Vec::new();

    for line in &reference {
        let got = analyzer.analyze(&line.text).expect("分词失败");
        if got.len() != line.tokens.len() {
            count_mismatch += 1;
            if samples.len() < 5 {
                samples.push(format!(
                    "{:?}: token 数 {} vs {}",
                    line.text,
                    got.len(),
                    line.tokens.len()
                ));
            }
        }
        for (a, b) in got.iter().zip(line.tokens.iter()) {
            total += 1;
            let same = a.surface == b.surface
                && a.lemma == b.lemma
                && a.normalized == b.normalized
                && a.reading == b.reading
                && a.pos == b.pos
                && a.upos == b.upos;
            if same {
                identical += 1;
            } else if samples.len() < 5 {
                samples.push(format!(
                    "{:?}: Rust({} {} {:?}) vs Py({} {} {:?})",
                    line.text, a.surface, a.lemma, a.pos, b.surface, b.lemma, b.pos
                ));
            }
        }
    }

    let rate = identical as f64 / total.max(1) as f64;
    let threshold = baseline.thresholds.sudachi_rs_vs_sudachi_py_token_identity;
    println!(
        "第一级：{identical}/{total} 逐 token 一致 = {:.4}（阈值 {threshold}）\
         \n  行 token 数不一致：{count_mismatch}/{}",
        rate,
        reference.len()
    );
    for s in &samples {
        println!("  样本 {s}");
    }
    assert!(
        rate >= threshold,
        "sudachi.rs 与 SudachiPy 一致率 {rate:.4} 低于阈值 {threshold}——\
         多半是词典版本或 sudachi.json 配置不同"
    );
}

// ────────────────────────── 第二级验收 ──────────────────────────

#[test]
fn tier2_matches_the_existing_corpus() {
    let root = project_root();
    let path = root.join("benchmark/ginza_reference.jsonl");
    require!(path);
    let Some(baseline) = load_baseline(&root) else { return };
    let Some(analyzer) = analyzer(&root) else {
        eprintln!("跳过：找不到 venv 里的 SudachiPy 资源");
        return;
    };

    let reference: Vec<GinzaLine> = read_jsonl(&path);
    let (mut aligned, mut upos_ok, mut content_ok, mut lemma_ok) = (0usize, 0, 0, 0);
    let (mut lines, mut boundary_content_ok) = (0usize, 0usize);

    for line in &reference {
        let got = analyzer.analyze(&line.text).expect("分词失败");
        lines += 1;

        let g_surfaces: Vec<String> = line.tokens.iter().map(|t| t.surface.clone()).collect();
        let s_surfaces: Vec<String> = got.iter().map(|t| t.surface.clone()).collect();

        // 实词部分的分词边界（标点两边切法本就不同，比它没有意义）
        let g_content: Vec<&String> = line
            .tokens
            .iter()
            .filter(|t| !jp_tokenizer::is_ignored(&t.pos) && !t.surface.trim().is_empty())
            .map(|t| &t.surface)
            .collect();
        let s_content: Vec<&String> = got
            .iter()
            .filter(|t| !jp_tokenizer::is_ignored(&t.upos) && !t.surface.trim().is_empty())
            .map(|t| &t.surface)
            .collect();
        if g_content == s_content {
            boundary_content_ok += 1;
        }

        let g_by_span: HashMap<(usize, usize), &GinzaToken> = char_spans(&g_surfaces)
            .into_iter()
            .zip(line.tokens.iter())
            .collect();

        for (span, token) in char_spans(&s_surfaces).into_iter().zip(got.iter()) {
            let Some(gold) = g_by_span.get(&span) else { continue };
            if gold.surface != token.surface {
                continue;
            }
            aligned += 1;
            if token.upos == gold.pos {
                upos_ok += 1;
            }
            if jp_tokenizer::is_content_word(&token.upos)
                == jp_tokenizer::is_content_word(&gold.pos)
            {
                content_ok += 1;
            }
            if token.lemma == gold.lemma {
                lemma_ok += 1;
            }
        }
    }

    let a = aligned.max(1) as f64;
    let upos_rate = upos_ok as f64 / a;
    let content_rate = content_ok as f64 / a;
    let lemma_rate = lemma_ok as f64 / a;
    let boundary_rate = boundary_content_ok as f64 / lines.max(1) as f64;
    let t = &baseline.thresholds;

    println!(
        "第二级（对齐 {aligned} token / {lines} 行）：\
         \n  UPOS 一致        {upos_rate:.4}  阈值 {:.3}\
         \n  『是否实词』一致   {content_rate:.4}  阈值 {:.3}\
         \n  lemma 一致       {lemma_rate:.4}  阈值 {:.3}\
         \n  实词边界一致      {boundary_rate:.4}  阈值 {:.3}",
        t.upos_exact_min, t.content_binary_min, t.lemma_exact_min,
        t.boundary_identical_content_min
    );

    assert!(upos_rate >= t.upos_exact_min, "UPOS 一致率 {upos_rate:.4} 低于阈值");
    assert!(content_rate >= t.content_binary_min, "实词二分类 {content_rate:.4} 低于阈值");
    assert!(lemma_rate >= t.lemma_exact_min, "lemma 一致率 {lemma_rate:.4} 低于阈值");
    assert!(
        boundary_rate >= t.boundary_identical_content_min,
        "实词边界一致率 {boundary_rate:.4} 低于阈值"
    );
}

// ────────────────────────── 冒烟 ──────────────────────────

#[test]
fn smoke_tokenizes_japanese() {
    let root = project_root();
    let Some(analyzer) = analyzer(&root) else {
        eprintln!("跳过：找不到 venv 里的 SudachiPy 资源");
        return;
    };
    let tokens = analyzer.analyze("夜に駆ける").expect("分词失败");
    let surfaces: Vec<&str> = tokens.iter().map(|t| t.surface.as_str()).collect();
    assert_eq!(surfaces, vec!["夜", "に", "駆ける"]);
    assert_eq!(tokens[0].upos, "NOUN");
    assert_eq!(tokens[1].upos, "ADP");
    assert_eq!(tokens[2].upos, "VERB");
    assert_eq!(tokens[2].lemma, "駆ける");
}
