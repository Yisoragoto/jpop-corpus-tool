//! 字符串相似度。**这里有一个必须知道的坑。**
//!
//! Python 侧 `scraper/matching.py` 是这样写的：
//!
//! ```python
//! try:
//!     from rapidfuzz.distance import JaroWinkler as _JaroWinkler
//!     def _similar(a, b): return _JaroWinkler.normalized_similarity(a, b)
//! except ImportError:
//!     def _similar(a, b): return difflib.SequenceMatcher(None, a, b).ratio()
//! ```
//!
//! 两条分支是**完全不同的算法**（Jaro-Winkler vs Ratcliff-Obershelp），
//! 给出的分数差得很远：`"202newmix"` vs `"aoi"`，difflib 给 0.167，
//! Jaro-Winkler 给 0.0。装没装 rapidfuzz 决定了同一首歌是自动采纳、
//! 需要确认还是判定失败——而这件事没有任何地方记录下来。
//!
//! 实测这台机器上 rapidfuzz **没有安装**，所以现有库里的分数、
//! 用户看到的行为，全部出自 difflib。
//!
//! 所以这里两种都实现，默认走 [`ratio`]（difflib 那条），迁移不改变行为。
//! 要换成 Jaro-Winkler 是一个明确的决定，不该由「装没装某个包」来决定。

use std::collections::HashMap;

/// difflib `SequenceMatcher(None, a, b).ratio()` 的等价实现。
///
/// `2 * M / T`，M 是递归分解出的最长匹配块的总长度，T 是两串长度之和。
/// 按**字符**算，不是字节——日文串上差别很大。
pub fn difflib_ratio(a: &str, b: &str) -> f64 {
    let sa: Vec<char> = a.chars().collect();
    let sb: Vec<char> = b.chars().collect();
    let total = sa.len() + sb.len();
    if total == 0 {
        return 1.0; // difflib 对两个空串返回 1.0
    }
    let matcher = Matcher::new(&sa, &sb);
    let matches = matcher.total_matches();
    2.0 * matches as f64 / total as f64
}

struct Matcher<'a> {
    a: &'a [char],
    b: &'a [char],
    /// b 里每个字符出现的全部下标。autojunk 剔除过的不在里面。
    b2j: HashMap<char, Vec<usize>>,
}

impl<'a> Matcher<'a> {
    fn new(a: &'a [char], b: &'a [char]) -> Self {
        let mut b2j: HashMap<char, Vec<usize>> = HashMap::new();
        for (i, ch) in b.iter().enumerate() {
            b2j.entry(*ch).or_default().push(i);
        }
        // autojunk：difflib 默认开启。b 长度 ≥ 200 时，出现次数超过 1% 的
        // 字符被当成「太常见、不具区分度」，整个从索引里删掉。
        // 曲名很少这么长，但专辑名和拼接串会——不实现它就会在长串上分叉。
        let n = b.len();
        if n >= 200 {
            let ntest = n / 100 + 1;
            b2j.retain(|_, idxs| idxs.len() <= ntest);
        }
        Self { a, b, b2j }
    }

    /// 所有匹配块的长度之和。
    ///
    /// difflib 的 `get_matching_blocks` 还会排序、合并相邻块、再补一个
    /// 长度 0 的哨兵——那些都不改变总长度，`ratio()` 只用总长度，
    /// 所以这里省掉。
    fn total_matches(&self) -> usize {
        let mut total = 0usize;
        let mut queue = vec![(0usize, self.a.len(), 0usize, self.b.len())];
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let (i, j, k) = self.longest_match(alo, ahi, blo, bhi);
            if k == 0 {
                continue;
            }
            total += k;
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
        total
    }

    /// difflib 的 `find_longest_match`。
    ///
    /// 没有 junk 集合（`isjunk=None`），所以那两轮「junk 也算」的扩展
    /// 不会执行，只保留普通扩展。
    fn longest_match(
        &self,
        alo: usize,
        ahi: usize,
        blo: usize,
        bhi: usize,
    ) -> (usize, usize, usize) {
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();

        for i in alo..ahi {
            let mut newj2len: HashMap<usize, usize> = HashMap::new();
            if let Some(indices) = self.b2j.get(&self.a[i]) {
                for &j in indices {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = j
                        .checked_sub(1)
                        .and_then(|prev| j2len.get(&prev).copied())
                        .unwrap_or(0)
                        + 1;
                    newj2len.insert(j, k);
                    if k > bestsize {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            j2len = newj2len;
        }

        // 向两侧扩展。b2j 里被 autojunk 剔掉的字符仍可能在这里被并进来，
        // difflib 就是这么做的。
        while besti > alo && bestj > blo && self.a[besti - 1] == self.b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && self.a[besti + bestsize] == self.b[bestj + bestsize]
        {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    }
}

/// rapidfuzz 的 `JaroWinkler.normalized_similarity`。
///
/// Python 侧只有装了 rapidfuzz 才会走这条。留着是为了让「换算法」
/// 成为一个能测、能对比的决定，而不是取决于环境里装没装包。
pub fn jaro_winkler(a: &str, b: &str) -> f64 {
    strsim::jaro_winkler(a, b)
}

/// 打分用的相似度算法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Similarity {
    /// difflib 的 Ratcliff-Obershelp。**Python 侧实际在跑的就是它**
    /// （rapidfuzz 没装），所以默认用它，迁移不改变行为。
    #[default]
    Difflib,
    /// Jaro-Winkler。Python 侧装了 rapidfuzz 才会用。
    JaroWinkler,
}

impl Similarity {
    pub fn compute(self, a: &str, b: &str) -> f64 {
        match self {
            Self::Difflib => difflib_ratio(a, b),
            Self::JaroWinkler => jaro_winkler(a, b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difflib_matches_python_on_known_pairs() {
        // 期望值是拿 Python 的 difflib.SequenceMatcher 实跑出来的
        let cases: &[(&str, &str, f64)] = &[
            ("202newmix", "aoi", 0.166_666_666_666_666_66),
            ("202newmix", "asiankungfugeneration", 0.2),
            ("202newmix", "ham", 0.166_666_666_666_666_66),
        ];
        for (a, b, want) in cases {
            let got = difflib_ratio(a, b);
            assert!((got - want).abs() < 1e-12, "{a} vs {b}: {got} != {want}");
        }
    }

    #[test]
    fn identical_and_empty_edges() {
        assert_eq!(difflib_ratio("abc", "abc"), 1.0);
        assert_eq!(difflib_ratio("", ""), 1.0);
        assert_eq!(difflib_ratio("abc", ""), 0.0);
    }

    #[test]
    fn it_counts_characters_not_bytes() {
        // 日文一个字符三个字节，按字节算比例会完全不同
        let r = difflib_ratio("夜に駆ける", "夜に駆ける");
        assert_eq!(r, 1.0);
        let r = difflib_ratio("夜に駆ける", "夜");
        // 2*1/(5+1) = 0.3333
        assert!((r - 1.0 / 3.0).abs() < 1e-12, "{r}");
    }

    #[test]
    fn the_two_algorithms_really_do_differ() {
        // 这条钉住的是本次移植最大的坑：装没装 rapidfuzz 会改变判定
        let (a, b) = ("202newmix", "aoi");
        assert!(difflib_ratio(a, b) > 0.16);
        assert_eq!(jaro_winkler(a, b), 0.0);
    }
}
