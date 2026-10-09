#!/usr/bin/env python3
"""
JPOP 语料库挖词报告生成器
从 Anki collection.anki2 读取 "JPOP Corpus" 笔记，统计已挖词汇与学习进度。

用法：
  python generate_report.py [--db path/to/collection.anki2] [--output output/corpus_report.html]
"""

from __future__ import annotations
import argparse, glob, html, os, re, shutil, sqlite3, sys, tempfile
import datetime
from dataclasses import dataclass
from pathlib import Path

NOTE_TYPE   = "JPOP Corpus"
# This script lives in legacy/; the report goes to output/ at the repository root.
_HERE = Path(__file__).resolve().parent
OUTPUT_PATH = (_HERE.parent if _HERE.name == "legacy" else _HERE) / "output" / "corpus_report.html"

# Field indices in our note type (see _ensure_anki_model in gui.py)
F_EXPRESSION = 0
F_SOURCE     = 5
F_JLPT       = 6
F_FREQ       = 8

TAG_RE  = re.compile(r"<[^>]+>")
# Source field format: "ヨルシカ「春ひさぎ」、Vaundy「怪獣の花唄」"
SRC_RE  = re.compile(r"([^「、\s][^「]*?)「([^」]+)」")

_JLPT_ORDER  = ["N5", "N4", "N3", "N2", "N1", "未分级"]
_JLPT_COLOR  = {
    "N5": "#4caf50", "N4": "#2196f3", "N3": "#ff9800",
    "N2": "#e91e63", "N1": "#9c27b0", "未分级": "#bdbdbd",
}

# ─────────────────────────── data model ───────────────────────────

@dataclass
class Card:
    expression: str
    artist:     str      # primary artist parsed from Source
    song:       str      # primary song title parsed from Source
    source_raw: str      # raw Source field value
    jlpt:       str      # "N5"/"N4"/… or ""
    freq:       str      # JPDB freq string
    deck:       str
    studied:    bool     # reps > 0


# ─────────────────────────── Anki DB helpers ──────────────────────

def find_collection() -> Path:
    """Auto-detect collection.anki2 on Windows/Mac/Linux."""
    candidates: list[Path] = []

    # Windows
    appdata = os.environ.get("APPDATA", "")
    if appdata:
        candidates += [Path(p) for p in glob.glob(
            os.path.join(appdata, "Anki2", "*", "collection.anki2"))]

    # macOS
    home = Path.home()
    candidates += list((home / "Library/Application Support/Anki2").glob(
        "*/collection.anki2"))

    # Linux
    candidates += list((home / ".local/share/Anki2").glob(
        "*/collection.anki2"))

    # Current directory fallback
    local = Path("collection.anki2")
    if local.exists():
        candidates.append(local)

    # Filter to existing files; prefer non-"User 1" profiles
    existing = [p for p in candidates if p.exists()]
    if not existing:
        return None
    existing.sort(key=lambda p: (p.stat().st_size == 0, p.name))
    return existing[0]


def snapshot(db: Path) -> tuple[tempfile.TemporaryDirectory, Path]:
    """Copy DB to a temp dir so we don't touch the live Anki file."""
    td  = tempfile.TemporaryDirectory(prefix="jpop-corpus-snap-")
    dst = Path(td.name) / db.name
    shutil.copy2(db, dst)
    for suf in ("-wal", "-shm"):
        s = db.with_name(db.name + suf)
        if s.exists():
            shutil.copy2(s, dst.with_name(dst.name + suf))
    return td, dst


def clean_field(raw: str) -> str:
    return TAG_RE.sub("", html.unescape(raw or "")).strip()


def parse_source(src: str) -> tuple[str, str]:
    """Return (primary_artist, primary_song) from Source field."""
    src = clean_field(src)
    m   = SRC_RE.search(src)
    if m:
        return m.group(1).strip(), m.group(2).strip()
    # fallback: use raw value as artist
    return src or "(不明)", ""


def _register_unicase(conn: sqlite3.Connection) -> None:
    def unicase(a: str, b: str) -> int:
        l, r = (a or "").casefold(), (b or "").casefold()
        return (l > r) - (l < r)
    conn.create_collation("unicase", unicase)


def load_cards(db: Path, deck_filter: str = "") -> list[Card]:
    td, snap = snapshot(db)
    conn = None
    try:
        conn = sqlite3.connect(str(snap))
        _register_unicase(conn)
        conn.row_factory = sqlite3.Row

        # Find our note type
        row = conn.execute(
            "SELECT id FROM notetypes WHERE name=?", (NOTE_TYPE,)
        ).fetchone()
        if not row:
            names = [r[0] for r in conn.execute("SELECT name FROM notetypes")]
            raise SystemExit(
                f'找不到笔记类型 "{NOTE_TYPE}"。\n'
                f'Anki 中已有类型：{", ".join(names)}\n'
                f'请先通过 GUI 导出至少一张卡片以创建该类型。'
            )
        mid = row[0]

        rows = conn.execute("""
            SELECT c.reps, d.name AS deck, n.flds
            FROM cards c
            JOIN notes n ON n.id = c.nid
            JOIN decks d ON d.id = c.did
            WHERE n.mid = ?
        """, (mid,)).fetchall()

        cards: list[Card] = []
        df = deck_filter.lower().strip()
        for r in rows:
            if df and df not in r["deck"].lower():
                continue
            fields = r["flds"].split("\x1f")
            def f(i): return fields[i] if i < len(fields) else ""

            expr   = clean_field(f(F_EXPRESSION))
            src    = f(F_SOURCE)
            jlpt   = clean_field(f(F_JLPT))
            freq   = clean_field(f(F_FREQ))
            artist, song = parse_source(src)

            cards.append(Card(
                expression=expr,
                artist=artist,
                song=song,
                source_raw=clean_field(src),
                jlpt=jlpt if jlpt in ("N5","N4","N3","N2","N1") else "",
                freq=freq,
                deck=r["deck"],
                studied=r["reps"] > 0,
            ))
        return cards
    finally:
        if conn:
            conn.close()
        td.cleanup()


# ─────────────────────────── aggregation ──────────────────────────

def progress_rows(cards: list[Card], key_fn, top=25):
    """Return list of (label, studied, total) sorted by total desc."""
    totals:   dict[str, int]  = {}
    studieds: dict[str, int]  = {}
    for c in cards:
        k = key_fn(c)
        totals[k]   = totals.get(k, 0) + 1
        studieds[k] = studieds.get(k, 0) + (1 if c.studied else 0)
    rows = sorted(totals.items(), key=lambda x: -x[1])[:top]
    return [(lbl, studieds.get(lbl, 0), tot) for lbl, tot in rows]


# ─────────────────────────── HTML rendering ───────────────────────

def esc(s): return html.escape(str(s))


def render_progress_bars(rows: list[tuple[str, int, int]]) -> str:
    if not rows:
        return "<p style='color:var(--muted)'>（データなし）</p>"
    max_total = max(r[2] for r in rows) or 1
    out = []
    for label, studied, total in rows:
        pct_studied   = studied / total * 100
        pct_bar_total = total   / max_total * 100
        pct_bar_unstd = (total - studied) / max_total * 100
        out.append(
            f'<div class="bar-row">'
            f'<span class="bar-label" title="{esc(label)}">{esc(label)}</span>'
            f'<span class="bar-track">'
            f'<span class="bar-fill-s" style="width:{pct_bar_total - pct_bar_unstd:.2f}%"></span>'
            f'<span class="bar-fill-u" style="width:{pct_bar_unstd:.2f}%"></span>'
            f'</span>'
            f'<span class="bar-val">{studied}/{total}'
            f' <span class="muted">({pct_studied:.0f}%)</span></span>'
            f'</div>'
        )
    return "".join(out)


def render_jlpt_bars(cards: list[Card]) -> str:
    totals  = {k: 0 for k in _JLPT_ORDER}
    studied = {k: 0 for k in _JLPT_ORDER}
    for c in cards:
        k = c.jlpt if c.jlpt in _JLPT_ORDER else "未分级"
        totals[k]  += 1
        studied[k] += 1 if c.studied else 0

    # stacked overview bar
    grand = len(cards) or 1
    segs  = "".join(
        f'<div class="jlpt-seg" style="width:{totals[k]/grand*100:.1f}%;'
        f'background:{_JLPT_COLOR[k]}" '
        f'title="{k}: {totals[k]:,} 词"></div>'
        for k in _JLPT_ORDER if totals[k]
    )

    rows = []
    max_t = max(totals.values()) or 1
    for k in _JLPT_ORDER:
        tot = totals[k]
        if not tot: continue
        std = studied[k]
        col = _JLPT_COLOR[k]
        pct_bar   = tot / max_t * 100
        pct_unstd = (tot - std) / max_t * 100
        pct_done  = std / tot * 100
        rows.append(
            f'<div class="bar-row">'
            f'<span class="bar-label"><span class="dot" style="background:{col}"></span>{esc(k)}</span>'
            f'<span class="bar-track">'
            f'<span class="bar-fill-s" style="width:{pct_bar-pct_unstd:.2f}%"></span>'
            f'<span class="bar-fill-u" style="width:{pct_unstd:.2f}%"></span>'
            f'</span>'
            f'<span class="bar-val">{std}/{tot} <span class="muted">({pct_done:.0f}%)</span></span>'
            f'</div>'
        )
    return (
        f'<div class="jlpt-stack">{segs}</div>'
        f'<div class="bar-list" style="margin-top:12px">{"".join(rows)}</div>'
    )


def render_word_table(cards: list[Card]) -> str:
    # Deduplicate by expression, keep "studied" if any copy is studied
    seen: dict[str, Card] = {}
    for c in cards:
        if c.expression not in seen or c.studied:
            seen[c.expression] = c
    words = sorted(seen.values(), key=lambda c: (not c.studied, c.jlpt or "ZZ", c.expression))

    rows = []
    for c in words:
        col   = _JLPT_COLOR.get(c.jlpt, _JLPT_COLOR["未分级"])
        badge = (f'<span class="badge" style="background:{col}">{esc(c.jlpt)}</span>'
                 if c.jlpt else '<span style="color:var(--muted)">—</span>')
        status = ('✅' if c.studied else '⬜')
        blob   = f'{c.expression} {c.jlpt} {c.artist} {c.song} {status}'.lower()
        rows.append(
            f'<tr data-s="{esc(blob)}">'
            f'<td class="lem">{esc(c.expression)}</td>'
            f'<td class="cen">{status}</td>'
            f'<td class="cen">{badge}</td>'
            f'<td class="num">{esc(c.freq) if c.freq else "—"}</td>'
            f'<td class="src">{esc(c.artist)}</td>'
            f'<td class="src">{esc(c.song)}</td>'
            f'<td class="src" style="color:var(--muted)">{esc(c.deck)}</td>'
            f'</tr>'
        )
    return "".join(rows)


# ─────────────────────────── CSS / JS ─────────────────────────────

CSS = """
:root{--bg:#f7f3ea;--card:#fffdf8;--ink:#1d1b17;--muted:#6e6558;
      --line:#d9cfbf;--accent:#c96b3b;--bar-bg:#eee6d7;--r:12px;
      --studied:#c96b3b;--unstudied:#e8d9c8}
*,*::before,*::after{box-sizing:border-box;margin:0;padding:0}
body{background:var(--bg);color:var(--ink);
     font-family:"Segoe UI","Noto Sans JP","Hiragino Sans",sans-serif;
     font-size:15px;line-height:1.6}
.wrap{max-width:1200px;margin:0 auto;padding:32px 20px}

.hero{background:var(--card);border:1px solid var(--line);border-radius:var(--r);
      padding:32px 36px;margin-bottom:24px}
.hero h1{font-size:2.2rem;font-weight:800;letter-spacing:-.02em;margin-bottom:10px}
.hero p{color:var(--muted);max-width:760px;font-size:.93rem;margin-bottom:16px}
.chips{display:flex;flex-wrap:wrap;gap:8px}
.chip{background:var(--bg);border:1px solid var(--line);border-radius:100px;
      padding:4px 14px;font-size:.82rem;color:var(--muted)}

.summary{display:grid;grid-template-columns:repeat(auto-fit,minmax(155px,1fr));
         gap:14px;margin-bottom:24px}
.sc{background:var(--card);border:1px solid var(--line);border-radius:var(--r);padding:20px 22px}
.sc .val{font-size:2rem;font-weight:800;line-height:1.1}
.sc .lbl{font-size:.8rem;color:var(--muted);margin-top:4px}

.grid{display:grid;grid-template-columns:1fr 1fr;gap:16px;margin-bottom:24px}
@media(max-width:860px){.grid{grid-template-columns:1fr}}
.full{grid-column:1/-1}

.panel{background:var(--card);border:1px solid var(--line);border-radius:var(--r);padding:24px}
.ptitle{font-size:1.05rem;font-weight:700;margin-bottom:4px}
.pintro{font-size:.84rem;color:var(--muted);margin-bottom:16px}

.legend{display:flex;gap:18px;margin-bottom:14px;font-size:.82rem;color:var(--muted)}
.legend span{display:flex;align-items:center;gap:6px}
.legend b{display:inline-block;width:12px;height:12px;border-radius:3px}

.bar-list{display:flex;flex-direction:column;gap:6px}
.bar-row{display:grid;grid-template-columns:170px 1fr auto;align-items:center;gap:10px}
.bar-label{font-size:.86rem;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.bar-track{height:12px;background:var(--bar-bg);border-radius:6px;
           overflow:hidden;display:flex}
.bar-fill-s{height:100%;background:var(--studied)}
.bar-fill-u{height:100%;background:var(--unstudied)}
.bar-val{font-size:.82rem;white-space:nowrap;text-align:right;min-width:90px}
.muted{color:var(--muted)}

.dot{display:inline-block;width:10px;height:10px;border-radius:50%;
     margin-right:6px;vertical-align:middle}
.jlpt-stack{display:flex;height:18px;border-radius:9px;overflow:hidden;margin-bottom:4px}
.jlpt-seg{height:100%;cursor:default}

.badge{display:inline-block;font-size:.72rem;font-weight:700;color:#fff;
       padding:1px 8px;border-radius:100px}

.tpanel{margin-bottom:24px}
.tsearch{width:100%;padding:10px 14px;border:1px solid var(--line);border-radius:8px;
         background:var(--bg);color:var(--ink);font-size:.92rem;margin-bottom:14px;outline:none}
.tsearch:focus{border-color:var(--accent)}
table{width:100%;border-collapse:collapse;font-size:.88rem}
th{text-align:left;padding:8px 12px;border-bottom:2px solid var(--line);
   color:var(--muted);font-weight:600;font-size:.78rem;text-transform:uppercase;
   letter-spacing:.04em;white-space:nowrap;cursor:pointer;user-select:none}
th:hover{color:var(--ink)}
th.sort-desc::after{content:" ↓"}th.sort-asc::after{content:" ↑"}
td{padding:7px 12px;border-bottom:1px solid var(--bar-bg);vertical-align:middle}
tr:hover td{background:var(--bg)}
.lem{font-weight:600;font-size:1rem}
.num{text-align:right;font-variant-numeric:tabular-nums;color:var(--muted)}
.cen{text-align:center}
.src{font-size:.85rem;color:var(--muted)}
.hidden{display:none!important}
"""

JS = r"""
const search = document.getElementById('ts');
const tbody  = document.getElementById('tb');
search.addEventListener('input', () => {
  const q = search.value.toLowerCase();
  for (const tr of tbody.rows)
    tr.classList.toggle('hidden', q.length > 0 && !(tr.dataset.s||'').includes(q));
});
let sc = 0, sa = true;
function sortBy(ci){
  sa = sc===ci ? !sa : (sc=ci, true);
  document.querySelectorAll('th').forEach((th,i)=>{
    th.classList.remove('sort-asc','sort-desc');
    if(i===sc) th.classList.add(sa?'sort-asc':'sort-desc');
  });
  const rows=[...tbody.rows].sort((a,b)=>{
    const av=a.cells[ci].textContent.replace(/[,，—✅⬜\s]/g,'');
    const bv=b.cells[ci].textContent.replace(/[,，—✅⬜\s]/g,'');
    const an=parseFloat(av),bn=parseFloat(bv);
    const c=(!isNaN(an)&&!isNaN(bn))?an-bn:av.localeCompare(bv,'ja');
    return sa?c:-c;
  });
  rows.forEach(r=>tbody.appendChild(r));
}
document.querySelectorAll('th').forEach((th,i)=>th.addEventListener('click',()=>sortBy(i)));
"""


# ─────────────────────────── build HTML ───────────────────────────

def build_html(cards: list[Card], db_path: Path) -> str:
    now      = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
    total    = len(cards)
    n_std    = sum(1 for c in cards if c.studied)
    n_expr   = len({c.expression for c in cards})
    n_artist = len({c.artist for c in cards})
    n_song   = len({(c.artist, c.song) for c in cards if c.song})
    n_deck   = len({c.deck for c in cards})
    pct_std  = n_std / total * 100 if total else 0

    # summary
    summary = (
        f'<div class="sc"><div class="val">{n_expr:,}</div>'
        f'<div class="lbl">已挖词汇</div></div>'

        f'<div class="sc"><div class="val">{n_std:,}'
        f'<span style="font-size:1rem;font-weight:400;color:var(--muted)"> ({pct_std:.0f}%)</span></div>'
        f'<div class="lbl">已学卡片</div></div>'

        f'<div class="sc"><div class="val">{total:,}</div>'
        f'<div class="lbl">总卡片数</div></div>'

        f'<div class="sc"><div class="val">{n_artist}</div>'
        f'<div class="lbl">歌手数</div></div>'

        f'<div class="sc"><div class="val">{n_song}</div>'
        f'<div class="lbl">歌曲数</div></div>'

        + (f'<div class="sc"><div class="val">{n_deck}</div>'
           f'<div class="lbl">牌组数</div></div>' if n_deck > 1 else "")
    )

    # legend
    legend = """
    <div class="legend">
      <span><b style="background:var(--studied)"></b>已学</span>
      <span><b style="background:var(--unstudied)"></b>未学</span>
    </div>"""

    # progress bars
    artist_rows = progress_rows(cards, lambda c: c.artist)
    song_rows   = progress_rows(cards, lambda c: f"{c.artist}「{c.song}」" if c.song else c.artist)
    a_bars = legend + '<div class="bar-list">' + render_progress_bars(artist_rows) + '</div>'
    s_bars = legend + '<div class="bar-list">' + render_progress_bars(song_rows)   + '</div>'

    # JLPT
    jlpt_html = render_jlpt_bars(cards)

    # word table
    tbl_rows = render_word_table(cards)

    return f"""<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>JPOP 语料库挖词报告</title>
<style>{CSS}</style>
</head>
<body>
<div class="wrap">

  <section class="hero">
    <h1>🎵 JPOP 语料库挖词报告</h1>
    <p>统计从 Anki 牌组中已挖出的 JPOP 词汇及学习进度。
    右侧数值显示为 已学/已挖（百分比）。</p>
    <div class="chips">
      <span class="chip">笔记类型：{esc(NOTE_TYPE)}</span>
      <span class="chip">来源：{esc(str(db_path))}</span>
      <span class="chip">生成时间：{now}</span>
    </div>
  </section>

  <section class="summary">{summary}</section>

  <section class="grid">

    <article class="panel">
      <div class="ptitle">歌手 / 作品进度</div>
      <div class="pintro">各歌手的已挖词数，深色为已学，浅色为未学。</div>
      {a_bars}
    </article>

    <article class="panel">
      <div class="ptitle">歌曲进度</div>
      <div class="pintro">各歌曲的已挖词数及学习进度。</div>
      {s_bars}
    </article>

    <article class="panel full">
      <div class="ptitle">JLPT 分布</div>
      <div class="pintro">
        已挖词汇的 JLPT 级别分布及各级别学习进度。
        深色为已学，浅色为未学。
      </div>
      {jlpt_html}
    </article>

  </section>

  <section class="panel tpanel">
    <div class="ptitle">词汇列表（共 {n_expr:,} 词）</div>
    <div class="pintro">
      ✅ = 已学（复习次数 &gt; 0），⬜ = 未学。点击列标题排序，输入框搜索过滤。
    </div>
    <input id="ts" class="tsearch" type="text"
           placeholder="搜索词汇、JLPT、歌手…">
    <table id="wt">
      <thead>
        <tr>
          <th>词汇</th><th>状态</th><th>JLPT</th>
          <th>JPDB 词频</th><th>歌手</th><th>歌曲</th><th>牌组</th>
        </tr>
      </thead>
      <tbody id="tb">{tbl_rows}</tbody>
    </table>
  </section>

</div>
<script>{JS}</script>
</body>
</html>"""


# ─────────────────────────── main ─────────────────────────────────

def main():
    ap = argparse.ArgumentParser(description="Generate JPOP corpus mining report from Anki")
    ap.add_argument("--db",           default=None,          help="Anki collection.anki2 path")
    ap.add_argument("--deck-contains",default="",            help="Only cards whose deck name contains this")
    ap.add_argument("--output",       default=str(OUTPUT_PATH), help="Output HTML path")
    args = ap.parse_args()

    # Resolve DB
    if args.db:
        db = Path(args.db).expanduser().resolve()
        if not db.exists():
            print(f"错误：找不到数据库 {db}", file=sys.stderr); sys.exit(1)
    else:
        db = find_collection()
        if not db:
            print(
                "错误：找不到 Anki collection.anki2\n"
                "请用 --db 参数指定路径，例如：\n"
                r'  python generate_report.py --db "C:\Users\你的用户名\AppData\Roaming\Anki2\User 1\collection.anki2"',
                file=sys.stderr
            )
            sys.exit(1)

    print(f"读取 {db} …")
    cards = load_cards(db, args.deck_contains)
    if not cards:
        print(
            f'错误：在 Anki 中找不到 "{NOTE_TYPE}" 类型的卡片。\n'
            f'请先通过 GUI 导出至少一张卡片。',
            file=sys.stderr
        )
        sys.exit(1)

    n_std = sum(1 for c in cards if c.studied)
    print(f"  找到 {len(cards)} 张卡片，{n_std} 张已学习")

    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(build_html(cards, db), encoding="utf-8")
    print(f"报告已生成：{out}")


if __name__ == "__main__":
    main()
