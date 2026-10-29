"""第一阶段语料分析指标的独立对账。

按论文公式重新实现一遍（不看 Rust 的写法，词流也从原始 token 重新切），
读同一个库，和 `cargo run -p jp-corpus --release --example analysis_dump -- <db> <json>`
的输出逐项比。只用标准库。

    python -I scripts/analysis_reconcile.py <corpus.db> <analysis_dump.json>

任何一项对不上都会列出来，最后打印每类比了多少项、最大误差。
"""

import json
import math
import sqlite3
import sys
import unicodedata
from collections import Counter, defaultdict

DB, DUMP = sys.argv[1], sys.argv[2]
TOL = 1e-9
problems = []
stats = defaultdict(lambda: [0, 0.0])  # 类别 → [比了几项, 最大绝对误差]


def close(cat, label, a, b, tol=TOL):
    stats[cat][0] += 1
    if a is None or b is None:
        if a is not None or b is not None:
            problems.append(f"{cat} {label}: rust={a} python={b}")
        return
    err = abs(a - b)
    stats[cat][1] = max(stats[cat][1], err)
    if err > tol * max(1.0, abs(b)):
        problems.append(f"{cat} {label}: rust={a!r} python={b!r}")


def same(cat, label, a, b):
    stats[cat][0] += 1
    if a != b:
        problems.append(f"{cat} {label}: rust={a!r} python={b!r}")


def is_japanese(lemma):
    for c in lemma:
        o = ord(c)
        if 0x3041 <= o <= 0x3093 or 0x30A1 <= o <= 0x30F6 or 0xFF66 <= o <= 0xFF7F or 0x4E00 <= o <= 0x9FFF or 0x3400 <= o <= 0x4DBF:
            return True
    return False


con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
raw_rows = con.execute(
    "SELECT u.song_id, u.id, u.text, t.surface, t.lemma, COALESCE(t.pos,'') "
    "FROM utterances u JOIN tokens t ON t.utterance_id = u.id "
    "ORDER BY u.song_id, u.line_idx, u.id, t.token_idx"
).fetchall()

# 原始行：每行保留全部 token（含标点），去不去在各个分析里决定
raw_lines = []
for song, uid, text, surface, lemma, pos in raw_rows:
    if not raw_lines or raw_lines[-1]["uid"] != uid:
        raw_lines.append({"song": song, "uid": uid, "text": text, "toks": []})
    raw_lines[-1]["toks"].append((surface, lemma, pos))
seen = set()
for line in raw_lines:
    key = (line["song"], line["text"])
    line["repeat"] = key in seen
    seen.add(key)


def kept(tok, jp_only):
    surface, lemma, pos = tok
    if pos in ("PUNCT", "SYM"):
        return False
    return not jp_only or is_japanese(lemma)


def stream(unit, jp_only, songs=None):
    """计入的行：[(song, uid, [tok…])]"""
    out = []
    for line in raw_lines:
        if songs is not None and line["song"] not in songs:
            continue
        if unit == "distinctLines" and line["repeat"]:
            continue
        out.append((line["song"], line["uid"], [t for t in line["toks"] if kept(t, jp_only)]))
    return out


# ────────────── 离散度（Gries 2008；Lijffijt & Gries 2012；Savický & Hlaváčová 2002） ──────────────

def parts_of(lines):
    sizes = {}
    for song, _, toks in lines:
        if toks:
            sizes[song] = sizes.get(song, 0) + len(toks)
    return sizes


def dispersion(per_song, positions, sizes):
    total = sum(sizes.values())
    f = len(positions)
    parts = list(sizes)
    s = {p: sizes[p] / total for p in parts}
    dp = 0.5 * sum(abs(per_song.get(p, 0) / f - s[p]) for p in parts)
    dp_norm = dp / (1 - min(s.values()))
    n = len(parts)
    if n >= 2:
        rel = [per_song.get(p, 0) / sizes[p] for p in parts]
        mean = sum(rel) / n
        sd = math.sqrt(sum((r - mean) ** 2 for r in rel) / n)
        d = 1 - (sd / mean) / math.sqrt(n - 1)
    else:
        d = None
    pos = sorted(positions)
    v = total / f
    dist = [pos[i + 1] - pos[i] for i in range(f - 1)] + [pos[0] + total - pos[-1]]
    arf = sum(min(x, v) for x in dist) / v
    return dp, dp_norm, d, arf


def check_dispersion(cat, label, rust, ref):
    dp, dp_norm, d, arf = ref
    close(cat, f"{label} dp", rust["dp"], dp)
    close(cat, f"{label} dpNorm", rust["dpNorm"], dp_norm)
    close(cat, f"{label} juillandD", rust["juillandD"], d)
    close(cat, f"{label} arf", rust["arf"], arf)


dump = json.load(open(DUMP, encoding="utf-8"))

# ────────────── 词频 + 离散度 ──────────────
for case in dump["frequency"]:
    unit, jp = case["unit"], case["jpOnly"]
    lines = stream(unit, jp)
    sizes = parts_of(lines)
    per = defaultdict(lambda: {"songs": defaultdict(int), "pos": []})
    i = 0
    for song, _, toks in lines:
        for surface, lemma, pos in toks:
            e = per[(lemma, pos)]
            e["songs"][song] += 1
            e["pos"].append(i)
            i += 1
    items = case["result"]["items"]
    same("词频", f"{unit}/{jp} 词种数", len(items), len(per))
    for it in items:
        e = per[(it["lemma"], it["pos"])]
        label = f"{unit}/{jp} {it['lemma']}/{it['pos']}"
        same("词频", f"{label} freq", it["freq"], len(e["pos"]))
        same("词频", f"{label} songs", it["songs"], len(e["songs"]))
        check_dispersion("离散度", label, it["dispersion"], dispersion(e["songs"], e["pos"], sizes))

# ────────────── 搭配 ──────────────
FUNCTION = set()
for case in dump["collocations"]:
    node = case["node"]
    lines = stream("tokens", False)
    n_total = sum(len(t) for _, _, t in lines)
    corpus_freq = Counter((l, p) for _, _, toks in lines for _, l, p in toks)
    obs = Counter()
    slots = 0
    node_freq = 0
    for _, _, toks in lines:
        for i, (_, lemma, _) in enumerate(toks):
            if lemma != node:
                continue
            node_freq += 1
            for j in range(max(0, i - 3), min(len(toks), i + 4)):
                if j != i:
                    slots += 1
                    obs[(toks[j][1], toks[j][2])] += 1
    res = case["result"]
    same("搭配", f"{node} 节点频次", res["nodeFreq"], node_freq)
    same("搭配", f"{node} 搭配词数", len(res["collocates"]), len(obs))
    for c in res["collocates"]:
        key = (c["lemma"], c["pos"])
        o, fc = obs[key], corpus_freq[key]
        e = slots * fc / n_total
        a, b, cc, d = o, slots - o, fc - o, n_total - slots - (fc - o)
        g = 0.0
        rows, cols = (a + b, cc + d), (a + cc, b + d)
        for x, (r, k) in zip((a, b, cc, d), ((0, 0), (0, 1), (1, 0), (1, 1))):
            ex = rows[r] * cols[k] / n_total
            if x > 0 and ex > 0:
                g += x * math.log(x / ex)
        label = f"{node}→{c['lemma']}"
        same("搭配", f"{label} O", c["freq"], o)
        close("搭配", f"{label} logDice", c["logDice"], 14 + math.log2(2 * o / (node_freq + fc)))
        close("搭配", f"{label} MI", c["mi"], math.log2(o / e))
        close("搭配", f"{label} MI3", c["mi3"], math.log2(o ** 3 / e))
        close("搭配", f"{label} t", c["tScore"], (o - e) / math.sqrt(o))
        close("搭配", f"{label} z", c["zScore"], (o - e) / math.sqrt(e))
        close("搭配", f"{label} LL", c["logLikelihood"], 2 * g)
        close("搭配", f"{label} ΔP", c["deltaP"], a / (a + b) - cc / (cc + d))
        close("搭配", f"{label} ΔP反向", c["deltaPReverse"], a / (a + cc) - b / (b + d))

# ────────────── 子语料对比 ──────────────
recent = set(dump["recent"])
target = stream("distinctLines", False, recent)
reference = stream("distinctLines", False, {l["song"] for l in raw_lines} - recent)
tf = Counter((l, p) for _, _, toks in target for _, l, p in toks)
rf = Counter((l, p) for _, _, toks in reference for _, l, p in toks)
tn = sum(len(t) for _, _, t in target)
rn = sum(len(t) for _, _, t in reference)
ref_items = {}
for key in set(tf) | set(rf):
    a, b = tf[key], rf[key]
    e1, e2 = tn * (a + b) / (tn + rn), rn * (a + b) / (tn + rn)
    ll = 2 * ((a * math.log(a / e1) if a else 0) + (b * math.log(b / e2) if b else 0))
    ref_items[key] = {
        "ll": ll if a / tn >= b / rn else -ll,
        "p": math.erfc(math.sqrt(ll / 2)) if ll > 0 else 1.0,
        "bic": ll - math.log(tn + rn),
        "diff": (a / tn - b / rn) / (b / rn) * 100 if b else None,
        "lr": math.log2((max(a, 0.5) / tn) / (max(b, 0.5) / rn)),
    }
for case in dump["keyness"]:
    res = case["result"]
    tag = f"maxP={case['maxP']} bonf={case['bonferroni']}"
    same("对比", f"{tag} tested", res["tested"], len(ref_items))
    threshold = None
    if case["maxP"]:
        threshold = case["maxP"] / len(ref_items) if case["bonferroni"] else case["maxP"]
    want = {k for k, v in ref_items.items() if v["ll"] != 0 and (threshold is None or v["p"] <= threshold)}
    got = res["over"] + res["under"]
    same("对比", f"{tag} 列出词数", len(got), len(want))
    for k in got:
        r = ref_items[(k["lemma"], k["pos"])]
        label = f"{tag} {k['lemma']}"
        close("对比", f"{label} LL", k["logLikelihood"], r["ll"])
        close("对比", f"{label} log ratio", k["logRatio"], r["lr"])
        # erfc 用的是 Numerical Recipes 近似，相对误差 < 1.2e-7
        close("对比 p 值", f"{label} p", k["pValue"], r["p"], tol=2e-7)
        close("对比", f"{label} BIC", k["bic"], r["bic"])
        close("对比", f"{label} %DIFF", k["percentDiff"], r["diff"])

# ────────────── N-gram ──────────────
for case in dump["ngrams"]:
    n, field, jp = case["n"], case["field"], case["jpOnly"]
    fi = 1 if field == "lemma" else 0
    lines = stream("tokens", jp)
    sizes = parts_of(lines)
    # 独立切法：在**原始** token 序列里找连续 n 个都保留的词（中间没有被去掉的）
    per = defaultdict(lambda: {"songs": defaultdict(int), "pos": []})
    i = 0
    for line in raw_lines:
        toks = line["toks"]
        keep = [kept(t, jp) for t in toks]
        kept_index = []
        for k in keep:
            kept_index.append(i if k else None)
            if k:
                i += 1
        for s in range(len(toks) - n + 1):
            if all(keep[s:s + n]):
                key = tuple(t[fi] for t in toks[s:s + n])
                per[key]["songs"][line["song"]] += 1
                per[key]["pos"].append(kept_index[s])
    items = case["result"]["items"]
    tag = f"n={n} {field} jp={jp}"
    same("N-gram", f"{tag} 种数", case["result"]["types"], len(per))
    same("N-gram", f"{tag} 列出", len(items), len(per))
    for it in items:
        e = per[tuple(it["words"])]
        label = f"{tag} {'+'.join(it['words'])}"
        same("N-gram", f"{label} freq", it["freq"], len(e["pos"]))
        same("N-gram", f"{label} songs", it["songs"], len(e["songs"]))
        check_dispersion("N-gram 离散度", label, it["dispersion"], dispersion(e["songs"], e["pos"], sizes))

# ────────────── 词汇丰富度、文字种 ──────────────

def mattr(toks, w):
    if len(toks) < w:
        return None
    return sum(len(set(toks[i:i + w])) / w for i in range(len(toks) - w + 1)) / (len(toks) - w + 1)


def mtld_pass(toks):
    factors, types, count = 0.0, set(), 0
    for t in toks:
        count += 1
        types.add(t)
        if len(types) / count <= 0.72:
            factors += 1
            types, count = set(), 0
    if count:
        factors += (1 - len(types) / count) / (1 - 0.72)
    return len(toks) / factors if factors > 0 else None


def mtld(toks):
    a, b = mtld_pass(toks), mtld_pass(toks[::-1])
    return None if a is None or b is None else (a + b) / 2


def script(c):
    if c in "々〆〇":
        return "kanji"
    name = unicodedata.name(c, "")
    if name.startswith(("CJK UNIFIED IDEOGRAPH", "CJK COMPATIBILITY IDEOGRAPH")):
        return "kanji"
    if "HIRAGANA" in name and "KATAKANA" not in name:
        return "hiragana"
    # 「゛」「゜」叫 KATAKANA-HIRAGANA …，在平假名区块里
    if name.startswith("KATAKANA-HIRAGANA") and 0x3041 <= ord(c) <= 0x309F:
        return "hiragana"
    if "KATAKANA" in name:
        return "katakana"
    if "LATIN" in name and unicodedata.category(c).startswith("L"):
        return "latin"
    return "other"


credits = defaultdict(set)
for song, person in con.execute("SELECT song_id, person_id FROM track_credits WHERE role = 'performer'"):
    credits[song].add(str(person))

for case in dump["lexical"]:
    group, unit = case["group"], case["unit"]
    lines = stream(unit, False)
    groups = defaultdict(lambda: {"lemmas": [], "chars": Counter(), "songs": set()})
    for song, _, toks in lines:
        keys = {"whole": [""], "song": [song], "performer": sorted(credits.get(song, []))}[group]
        for k in keys:
            g = groups[k]
            g["songs"].add(song)
            for surface, lemma, _ in toks:
                g["lemmas"].append(lemma)
                for c in surface:
                    g["chars"][script(c)] += 1
    rows = case["result"]["rows"]
    tag = f"{group}/{unit}"
    same("丰富度", f"{tag} 组数", len(rows), len(groups))
    for row in rows:
        g = groups[row["key"]]
        toks = g["lemmas"]
        label = f"{tag} {row['label'] or '整体'}"
        freq = Counter(toks)
        n = len(toks)
        same("丰富度", f"{label} tokens", row["tokens"], n)
        same("丰富度", f"{label} songs", row["songs"], len(g["songs"]))
        same("丰富度", f"{label} types", row["types"], len(freq))
        close("丰富度", f"{label} TTR", row["ttr"], len(freq) / n)
        close("丰富度", f"{label} MATTR", row["mattr"], mattr(toks, 50))
        close("丰富度", f"{label} MTLD", row["mtld"], mtld(toks))
        spectrum = Counter(freq.values())
        close("丰富度", f"{label} Yule K", row["yuleK"], 1e4 * (sum(m * m * v for m, v in spectrum.items()) - n) / n / n)
        close("丰富度", f"{label} hapax", row["hapaxRatio"], spectrum[1] / len(freq))
        chars = sum(g["chars"].values())
        same("文字种", f"{label} chars", row["chars"], chars)
        for k in ("kanji", "hiragana", "katakana", "latin", "other"):
            close("文字种", f"{label} {k}", row["scripts"][k], g["chars"][k] / chars)

# ────────────── 活用形、表记变体（第二阶段；库要先补过形态信息） ──────────────

morph_rows = con.execute(
    "SELECT u.id, t.surface, t.lemma, COALESCE(t.pos,''), COALESCE(t.reading,''), COALESCE(t.normalized,''), "
    "COALESCE(t.ctype,''), COALESCE(t.cform,'') FROM utterances u JOIN tokens t ON t.utterance_id = u.id "
    "ORDER BY u.song_id, u.line_idx, u.id, t.token_idx"
).fetchall()
morph_by_line = defaultdict(list)
for uid, *rest in morph_rows:
    morph_by_line[uid].append(rest)
morph_tokens = []  # (song, uid, surface, lemma, pos, reading, normalized, ctype, cform)，只要计入的
for line in raw_lines:
    for surface, lemma, pos, reading, normalized, ctype, cform in morph_by_line[line["uid"]]:
        if pos in ("PUNCT", "SYM"):
            continue
        morph_tokens.append((line["song"], line["uid"], surface, lemma, pos, reading, normalized, ctype, cform))

if dump.get("inflection"):
    forms = defaultdict(lambda: {"freq": 0, "songs": set(), "lemmas": Counter()})
    for song, _, surface, lemma, pos, reading, normalized, ctype, cform in morph_tokens:
        if ctype and cform:
            f = forms[cform]
            f["freq"] += 1
            f["songs"].add(song)
            f["lemmas"][lemma] += 1
    total = sum(f["freq"] for f in forms.values())
    got = dump["inflection"]
    same("活用形", "分母", got["total"], total)
    same("活用形", "种数", len(got["forms"]), len(forms))
    for f in got["forms"]:
        want = forms[f["cform"]]
        same("活用形", f"{f['cform']} freq", f["freq"], want["freq"])
        same("活用形", f"{f['cform']} songs", f["songs"], len(want["songs"]))
        close("活用形", f"{f['cform']} share", f["share"], want["freq"] / total)
        top = sorted(want["lemmas"].items(), key=lambda x: (-x[1], x[0]))[:6]
        same("活用形", f"{f['cform']} 例词", [tuple(x) for x in f["lemmas"]], top)

if dump.get("orthography"):
    def writing(text):
        kinds = {script(c) for c in text}
        if "kanji" in kinds:
            return "kanji"
        if kinds == {"hiragana"}:
            return "hiragana"
        if kinds == {"katakana"}:
            return "katakana"
        if kinds == {"hiragana", "katakana"}:
            return "kana"
        return "other"

    groups = {}
    function_pos = {"ADP", "AUX", "SCONJ", "CCONJ", "PART", "DET"}
    for song, uid, surface, lemma, pos, reading, normalized, ctype, cform in morph_tokens:
        if not normalized or pos in function_pos:
            continue
        # 活用型也进键：行ける（下一段）不和 行く（五段）算一组
        g = groups.setdefault((normalized, pos, ctype), {"forms": {}, "readings": Counter(), "inflecting": False})
        g["inflecting"] |= bool(ctype)
        g["readings"][reading] += 1
        f = g["forms"].setdefault(lemma, {"freq": 0, "songs": set()})
        f["freq"] += 1
        f["songs"].add(song)

    def main_reading(g):
        return min(g["readings"].items(), key=lambda x: (-x[1], x[0]))[0]

    kanji_groups = defaultdict(list)
    for key, g in groups.items():
        if not g["inflecting"] and writing(key[0]) == "kanji":
            kanji_groups[(main_reading(g), key[1], key[2])].append(key)
    merged = set()
    for key in [k for k, g in groups.items() if not g["inflecting"] and writing(k[0]) in ("hiragana", "katakana", "kana")]:
        targets = kanji_groups.get((main_reading(groups[key]), key[1], key[2]), [])
        if len(targets) != 1:
            continue
        src = groups.pop(key)
        dst = groups[targets[0]]
        for lemma, f in src["forms"].items():
            d = dst["forms"].setdefault(lemma, {"freq": 0, "songs": set()})
            d["freq"] += f["freq"]
            d["songs"] |= f["songs"]
        dst["readings"].update(src["readings"])
        merged.add(targets[0])
    want = {k: g for k, g in groups.items() if len(g["forms"]) >= 2}
    got = dump["orthography"]
    same("表记变体", "组数", got["totalGroups"], len(want))
    # Rust 输出的组不带活用型：按（正规化表记，词性）配，同一键下可能有几组（行く / 行ける 各一组），按写法集合认
    want_by_np = defaultdict(list)
    for k, w in want.items():
        want_by_np[(k[0], k[1])].append((k, w))
    for g in got["groups"]:
        lemmas = {f["lemma"] for f in g["forms"]}
        match = [(k, w) for k, w in want_by_np.get((g["normalized"], g["pos"]), []) if set(w["forms"]) == lemmas]
        if len(match) != 1:
            problems.append(f"表记变体 对不上 {(g['normalized'], g['pos'])} {sorted(lemmas)}")
            continue
        key, w = match[0]
        forms_want = sorted(((l, f["freq"], len(f["songs"])) for l, f in w["forms"].items()), key=lambda x: (-x[1], x[0]))
        same("表记变体", f"{key} 写法", [(f["lemma"], f["freq"], f["songs"]) for f in g["forms"]], forms_want)
        same("表记变体", f"{key} 文字", [f["writing"] for f in g["forms"]], [writing(l) for l, _, _ in forms_want])
        same("表记变体", f"{key} 读音", g["reading"], main_reading(w))
        same("表记变体", f"{key} 并过", g["mergedByReading"], key in merged)

for cat, (count, err) in stats.items():
    print(f"{cat:12} {count:>9,} 项  最大绝对误差 {err:.3g}")
print(f"不一致 {len(problems)} 项")
for p in problems[:60]:
    print("  " + p)
sys.exit(1 if problems else 0)
