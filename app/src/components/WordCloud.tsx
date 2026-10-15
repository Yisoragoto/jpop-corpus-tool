/**
 * 统计页的词云：歌手照片重度虚化铺底，词浮在一块毛玻璃上。PyQt 版统计页「词云图」按钮的替代。
 *
 * 数据就是下面词频表那一份（同一组筛选：歌手、仅日文、词性），按词元合并后取前 120 个。
 * 每个词都能点：弹出语料库里出现它的歌（走检索页同一个 KWIC 接口），点歌直接跳到那一行。
 *
 * 鼠标动效全部绕开 React：指针一动就重渲染一百多个 span 会掉帧。
 * 位置、缩放用弹簧在 requestAnimationFrame 里算，只写 transform；静止了就停表。
 * 系统开了「减少动态效果」就不跑。
 *
 * 字体在设置页「词云」一组里选，可以是自带、导入或本机的任何一种。
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

import { api, type FrequencyRow, type PersonSummary } from "../api";
import { imageColor, toHighlight } from "../coverColor";
import { ensureFontFile, fetchFontCatalog, loadImportedFonts, lyricFontStack } from "../fonts";
import { pyGrouped } from "../pyFormat";
import { useAppSettings } from "../settings";
import { groupHitsBySong, layoutCloud, mergeByLemma, type CloudWord, type Placed, type SongHits } from "../wordCloud";
import { Avatar } from "./Avatar";
import { loadPerformers } from "./PerformerPicker";

/** 云里最多放多少个词。再多最小号的字就挤成噪点了 */
const CLOUD_WORDS = 120;
const CLOUD_HEIGHT = 440;
/** 弹出的歌曲列表：KWIC 最多取这么多行再按歌分组 */
const KWIC_LIMIT = 3000;
/** 底图最多拼几位歌手的照片 */
const BACKDROP_PEOPLE = 5;
/** 没有照片可取色时的高光色：界面的强调色 */
const DEFAULT_TINT = "rgb(159, 216, 255)";

interface Props {
  rows: FrequencyRow[] | null;
  /** 统计页筛选的演唱者，空 = 全部 */
  performerIds: number[];
  loading: boolean;
  /** 打开曲目并定位到那一行 */
  onOpenSong: (songId: string, utteranceId: number, timeSec: number | null) => void;
  /** 去检索页按词元查 */
  onSearchWord: (lemma: string) => void;
  /** 去曲库页看这个词的例句 */
  onShowExamples: (lemma: string) => void;
  onError: (message: string) => void;
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

/** 名次 → 颜色。前几名染上照片的颜色，其余白色按名次渐淡 */
function wordColor(rank: number, total: number, tint: string): { color: string; shadow: string } {
  if (rank < 6) return { color: tint, shadow: `0 0 24px ${withAlpha(tint, 0.4)}, 0 2px 10px rgba(0,0,0,.35)` };
  const alpha = 0.5 + 0.5 * Math.pow(1 - rank / Math.max(1, total), 1.4);
  return { color: `rgba(255,255,255,${alpha.toFixed(2)})`, shadow: "0 1px 8px rgba(0,0,0,.35)" };
}

function withAlpha(rgb: string, alpha: number): string {
  return rgb.replace(/^rgb\((.+)\)$/, `rgba($1, ${alpha})`);
}

/* ── 字体：设置里选的字体有文件就先加载，加载完再排布（量字宽要用真字体） ── */
function useCloudFont(family: string): { stack: string; ready: number } {
  const stack = useMemo(() => lyricFontStack(family, ""), [family]);
  const [ready, setReady] = useState(0);
  useEffect(() => {
    let alive = true;
    (async () => {
      if (family !== "") {
        try {
          const catalog = await fetchFontCatalog(loadImportedFonts());
          await Promise.all(catalog.files.filter((f) => f.family === family).map(ensureFontFile));
        } catch {
          // 加载不了就用兜底字体，不打断
        }
      }
      try {
        await document.fonts.load(`40px ${stack}`, "君僕夢");
      } catch {
        // 同上
      }
      if (alive) setReady((n) => n + 1);
    })();
    return () => {
      alive = false;
    };
  }, [family, stack]);
  return { stack, ready };
}

/* ── 弹簧动效 ── */
interface MotionItem {
  el: HTMLSpanElement;
  cx: number;
  cy: number;
  x: number;
  y: number;
  s: number;
  vx: number;
  vy: number;
  vs: number;
}

class CloudMotion {
  items: MotionItem[] = [];
  hot: MotionItem | null = null;
  /** 指针在云里的坐标；null = 不在 */
  pointer: { x: number; y: number } | null = null;
  /** 指针在整张卡上的位置，-1..1，背景视差用 */
  nx = 0;
  ny = 0;
  bg = { el: null as HTMLElement | null, x: 0, y: 0, vx: 0, vy: 0 };
  private raf = 0;

  kick() {
    if (this.raf === 0) this.raf = requestAnimationFrame(this.step);
  }

  stop() {
    cancelAnimationFrame(this.raf);
    this.raf = 0;
  }

  private step = () => {
    this.raf = 0;
    let busy = false;
    const spring = (o: Record<string, number>, key: string, target: number, k: number, damping: number) => {
      const vKey = "v" + key;
      const now = o[key] ?? 0;
      const v = ((o[vKey] ?? 0) + (target - now) * k) * damping;
      o[vKey] = v;
      o[key] = now + v;
      if (Math.abs(v) > 0.002 || Math.abs(target - now - v) > 0.002) busy = true;
    };
    const RADIUS = 150;
    const PUSH = 14;
    const LENS = 0.16;
    for (const it of this.items) {
      let tx = 0;
      let ty = 0;
      let ts = 1;
      if (this.pointer) {
        if (it === this.hot) ts = 1 + LENS * 1.6;
        else {
          const dx = it.cx - this.pointer.x;
          const dy = it.cy - this.pointer.y;
          const d = Math.hypot(dx, dy);
          if (d < RADIUS) {
            const f = (1 - d / RADIUS) ** 2;
            tx = (dx / (d || 1)) * PUSH * f;
            ty = (dy / (d || 1)) * PUSH * f;
            ts = 1 + LENS * f;
          }
        }
      }
      const o = it as unknown as Record<string, number>;
      spring(o, "x", tx, 0.16, 0.74);
      spring(o, "y", ty, 0.16, 0.74);
      spring(o, "s", ts, 0.2, 0.7);
      it.el.style.transform = `translate3d(${it.x.toFixed(2)}px, ${it.y.toFixed(2)}px, 0) scale(${it.s.toFixed(4)})`;
    }
    if (this.bg.el) {
      const o = this.bg as unknown as Record<string, number>;
      spring(o, "x", -this.nx * 26, 0.06, 0.82);
      spring(o, "y", -this.ny * 26, 0.06, 0.82);
      this.bg.el.style.translate = `${this.bg.x.toFixed(2)}px ${this.bg.y.toFixed(2)}px`;
    }
    if (busy) this.kick();
  };
}

/* ── 主组件 ── */
export function WordCloud({ rows, performerIds, loading, onOpenSong, onSearchWord, onShowExamples, onError }: Props) {
  const settings = useAppSettings();
  const { stack, ready } = useCloudFont(settings.wordCloudFont);
  const [people, setPeople] = useState<PersonSummary[]>([]);
  const [width, setWidth] = useState(0);
  const [tint, setTint] = useState(DEFAULT_TINT);
  const [picked, setPicked] = useState<{ word: CloudWord; x: number; y: number; tint: string } | null>(null);
  const [saving, setSaving] = useState(false);

  const stageRef = useRef<HTMLDivElement>(null);
  const glassRef = useRef<HTMLDivElement>(null);
  const cloudRef = useRef<HTMLDivElement>(null);
  const bgRef = useRef<HTMLDivElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const motion = useMemo(() => new CloudMotion(), []);
  const reduceMotion = useMemo(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches, []);

  useEffect(() => {
    let alive = true;
    loadPerformers()
      .then((list) => alive && setPeople(list))
      .catch((e) => onError(message(e)));
    return () => {
      alive = false;
    };
  }, [onError]);

  // 卡片上署谁、底图拼谁的照片：选了歌手就是选中的人，没选就是曲数最多、有照片的几位
  const chosen = useMemo(() => {
    const byId = new Map(people.map((p) => [p.id, p]));
    return performerIds.map((id) => byId.get(id)).filter((p): p is PersonSummary => p !== undefined);
  }, [people, performerIds]);
  const backdrop = useMemo(() => {
    const pool = chosen.length > 0 ? chosen : [...people].sort((a, b) => b.trackCount - a.trackCount);
    return pool.filter((p) => p.imagePath !== "").slice(0, BACKDROP_PEOPLE);
  }, [chosen, people]);

  // 高光色取第一张照片的主色
  const tintPath = backdrop[0]?.imagePath ?? "";
  useEffect(() => {
    let alive = true;
    setTint(DEFAULT_TINT);
    if (tintPath !== "") {
      void imageColor(tintPath).then((rgb) => {
        if (alive && rgb) setTint(toHighlight(rgb));
      });
    }
    return () => {
      alive = false;
    };
  }, [tintPath]);

  // 宽度变了才重新排（高度固定）。差几个像素不重排，免得拖窗口时闪
  useLayoutEffect(() => {
    const el = cloudRef.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const w = Math.round(entry.contentRect.width);
      setWidth((old) => (Math.abs(old - w) >= 24 ? w : old));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const words = useMemo(() => mergeByLemma(rows ?? [], CLOUD_WORDS), [rows]);
  const placed = useMemo(() => {
    if (width === 0 || ready === 0) return [];
    const ctx = document.createElement("canvas").getContext("2d");
    const measure = (text: string, size: number) => {
      if (!ctx) return [...text].length * size;
      ctx.font = `${size}px ${stack}`;
      return ctx.measureText(text).width;
    };
    return layoutCloud(words, {
      width,
      height: CLOUD_HEIGHT,
      minFont: 13,
      maxFont: Math.min(80, width / 8.5),
      verticalRatio: 0.22,
      measure,
    });
  }, [words, width, stack, ready]);

  // 每次排完，把动效的对象表换成新的 span
  useEffect(() => {
    const cloud = cloudRef.current;
    if (!cloud) return;
    motion.hot = null;
    const spans = [...cloud.querySelectorAll<HTMLSpanElement>(".wc-word")];
    motion.items = placed.flatMap((p, i) => {
      const el = spans[i];
      if (!el) return [];
      el.style.transform = "";
      return [{ el, cx: p.x + p.w / 2, cy: p.y + p.h / 2, x: 0, y: 0, s: 1, vx: 0, vy: 0, vs: 0 }];
    });
    motion.bg.el = bgRef.current;
  }, [placed, motion]);
  useEffect(() => () => motion.stop(), [motion]);

  const onPointerMove = (e: React.PointerEvent) => {
    const stage = stageRef.current;
    const cloud = cloudRef.current;
    const glass = glassRef.current;
    if (!stage || !cloud || !glass) return;
    const sr = stage.getBoundingClientRect();
    const cr = cloud.getBoundingClientRect();
    const gr = glass.getBoundingClientRect();
    glass.style.setProperty("--mx", `${e.clientX - gr.left}px`);
    glass.style.setProperty("--my", `${e.clientY - gr.top}px`);
    glass.style.setProperty("--lit", "1");
    if (reduceMotion) return;
    motion.pointer = { x: e.clientX - cr.left, y: e.clientY - cr.top };
    motion.nx = ((e.clientX - sr.left) / sr.width) * 2 - 1;
    motion.ny = ((e.clientY - sr.top) / sr.height) * 2 - 1;
    motion.kick();
  };
  const onPointerLeave = () => {
    glassRef.current?.style.setProperty("--lit", "0");
    motion.pointer = null;
    motion.nx = motion.ny = 0;
    motion.kick();
  };

  const showTip = (p: Placed, e: React.PointerEvent) => {
    const tip = tipRef.current;
    if (!tip) return;
    tip.textContent = `${p.word.lemma} · ${pyGrouped(p.word.freq)} 次 · ${pyGrouped(p.word.songCount)} 首 · 点开看是哪些歌`;
    tip.hidden = false;
    moveTip(e);
  };
  const moveTip = (e: React.PointerEvent) => {
    const tip = tipRef.current;
    if (!tip) return;
    tip.style.left = `${Math.min(e.clientX + 14, window.innerWidth - tip.offsetWidth - 8)}px`;
    tip.style.top = `${e.clientY + 16}px`;
  };
  const hideTip = () => {
    if (tipRef.current) tipRef.current.hidden = true;
  };

  const enter = (i: number, p: Placed, e: React.PointerEvent) => {
    const item = motion.items[i];
    if (item) {
      motion.hot = item;
      item.el.classList.add("hot");
    }
    cloudRef.current?.classList.add("hovering");
    showTip(p, e);
    motion.kick();
  };
  const leave = (i: number) => {
    const item = motion.items[i];
    if (item) {
      if (motion.hot === item) motion.hot = null;
      item.el.classList.remove("hot");
    }
    cloudRef.current?.classList.remove("hovering");
    hideTip();
    motion.kick();
  };

  const click = (i: number, p: Placed, e: React.MouseEvent) => {
    hideTip();
    const item = motion.items[i];
    const cloud = cloudRef.current;
    if (item && cloud && !reduceMotion) {
      // 点下去先缩一下再弹回来，同时从词心扩散一圈波纹
      const inner = item.el.firstElementChild as HTMLElement | null;
      inner?.classList.remove("picked");
      void inner?.offsetWidth;
      inner?.classList.add("picked");
      const ripple = document.createElement("span");
      ripple.className = "wc-ripple";
      const d = Math.max(p.w, p.h) * 0.9;
      Object.assign(ripple.style, { left: `${item.cx}px`, top: `${item.cy}px`, width: `${d}px`, height: `${d}px` });
      cloud.appendChild(ripple);
      window.setTimeout(() => ripple.remove(), 900);
    }
    setPicked({ word: p.word, x: e.clientX, y: e.clientY, tint: p.rank < 6 ? tint : "rgb(255,255,255)" });
  };

  const who =
    chosen.length === 0 ? "全部歌手" : chosen.length <= 3 ? chosen.map((p) => p.name).join("、") : `${chosen.slice(0, 3).map((p) => p.name).join("、")} 等 ${chosen.length} 位`;
  const songTotal = chosen.length === 0 ? null : chosen.reduce((n, p) => n + p.trackCount, 0);

  const savePng = async () => {
    setSaving(true);
    try {
      const path = await save({
        title: "保存词云",
        defaultPath: "wordcloud.png",
        filters: [{ name: "PNG 图片", extensions: ["png"] }],
      });
      if (!path) return;
      const bytes = await renderPng({ placed, width, height: CLOUD_HEIGHT, stack, tint, title: who, photos: backdrop.map((p) => p.imagePath) });
      await api.exportPng(path, bytes);
    } catch (err) {
      onError(`词云没能保存：${message(err)}`);
    } finally {
      setSaving(false);
    }
  };

  const empty = rows !== null && words.length === 0;

  return (
    <section className="card">
      <div
        className="wc-stage"
        ref={stageRef}
        onPointerMove={onPointerMove}
        onPointerLeave={onPointerLeave}
        style={{ ["--wc-tint" as string]: tint }}
      >
        <div className="wc-bg" ref={bgRef} aria-hidden="true">
          {backdrop.length === 0 && <div className="wc-bg-plain" />}
          {backdrop.map((p, i) => (
            <img
              key={p.id}
              src={convertFileSrc(p.imagePath)}
              alt=""
              draggable={false}
              style={backdrop.length === 1 ? undefined : mosaicSpot(i, backdrop.length)}
              className={backdrop.length === 1 ? "single" : undefined}
            />
          ))}
        </div>
        <div className="wc-dim" aria-hidden="true" />

        <div className="wc-head">
          <div className="wc-faces">
            {chosen.length === 0 && backdrop.length === 0 && <span className="avatar avatar-text wc-all">全</span>}
            {(chosen.length > 0 ? chosen : backdrop).slice(0, 3).map((p) => (
              <Avatar key={p.id} path={p.imagePath} name={p.name} size={chosen.length === 1 ? 56 : 40} />
            ))}
          </div>
          <div className="wc-who">
            <h2>{who}</h2>
            <span>
              {songTotal !== null && `${pyGrouped(songTotal)} 首 · `}
              {loading ? "读取中…" : `前 ${placed.length} 个词`}
              {" · 点词看出现在哪些歌"}
            </span>
          </div>
          <div className="wc-tools">
            <button className="wc-btn" disabled={placed.length === 0 || saving} onClick={() => void savePng()}>
              {saving ? "保存中…" : "保存 PNG"}
            </button>
          </div>
        </div>

        <div className="wc-glass" ref={glassRef}>
          <div
            className={`wc-cloud${loading ? " stale" : ""}`}
            ref={cloudRef}
            style={{ height: CLOUD_HEIGHT, fontFamily: stack }}
            lang="ja"
          >
            {placed.map((p, i) => {
              const c = wordColor(p.rank, words.length, tint);
              return (
                <span
                  key={`${p.word.lemma}:${width}:${stack}`}
                  className={`wc-word${p.vertical ? " v" : ""}`}
                  style={{ left: p.x, top: p.y, width: p.w, height: p.h, ["--glow" as string]: p.rank < 6 ? tint : "rgba(255,255,255,.55)" }}
                  role="button"
                  tabIndex={0}
                  aria-label={`${p.word.lemma}，${p.word.freq} 次，打开歌曲列表`}
                  onPointerEnter={(e) => enter(i, p, e)}
                  onPointerMove={moveTip}
                  onPointerLeave={() => leave(i)}
                  onClick={(e) => click(i, p, e)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                      setPicked({ word: p.word, x: r.left + r.width / 2, y: r.top + r.height / 2, tint: p.rank < 6 ? tint : "rgb(255,255,255)" });
                    }
                  }}
                >
                  <span
                    className="wc-inner"
                    style={{
                      fontSize: p.fontSize,
                      color: c.color,
                      textShadow: c.shadow,
                      animationDelay: reduceMotion ? undefined : `${Math.min(i * 8, 700)}ms`,
                    }}
                  >
                    {p.word.lemma}
                  </span>
                </span>
              );
            })}
          </div>
          {empty && <p className="wc-empty">这组筛选下没有词</p>}
        </div>
      </div>
      <div className="wc-tip" ref={tipRef} hidden />

      {picked && (
        <SongPopover
          key={`${picked.word.lemma}:${picked.x}:${picked.y}`}
          word={picked.word}
          x={picked.x}
          y={picked.y}
          highlight={picked.tint}
          stack={stack}
          performerIds={performerIds}
          people={people}
          onClose={() => setPicked(null)}
          onOpenSong={(...args) => {
            setPicked(null);
            onOpenSong(...args);
          }}
          onSearchWord={(lemma) => {
            setPicked(null);
            onSearchWord(lemma);
          }}
          onShowExamples={(lemma) => {
            setPicked(null);
            onShowExamples(lemma);
          }}
          onError={onError}
        />
      )}
    </section>
  );
}

/** 几张照片拼底图：铺成两行，互相盖一点，糊开之后边界就看不出来了 */
function mosaicSpot(i: number, n: number): React.CSSProperties {
  const spots: [number, number][] = [
    [0, 0],
    [50, 0],
    [0, 50],
    [50, 50],
    [25, 25],
  ];
  const pair: [number, number][] = [
    [0, 0],
    [50, 25],
  ];
  const [left, top] = (n === 2 ? pair[i] : spots[i]) ?? [25, 25];
  return { left: `${left}%`, top: `${top}%`, width: "55%", height: "55%" };
}

/* ── 点词弹出：出现这个词的歌 ── */
function SongPopover({
  word,
  x,
  y,
  highlight,
  stack,
  performerIds,
  people,
  onClose,
  onOpenSong,
  onSearchWord,
  onShowExamples,
  onError,
}: {
  word: CloudWord;
  x: number;
  y: number;
  highlight: string;
  stack: string;
  performerIds: number[];
  people: PersonSummary[];
  onClose: () => void;
  onOpenSong: (songId: string, utteranceId: number, timeSec: number | null) => void;
  onSearchWord: (lemma: string) => void;
  onShowExamples: (lemma: string) => void;
  onError: (message: string) => void;
}) {
  const [songs, setSongs] = useState<SongHits[] | null>(null);
  const [truncated, setTruncated] = useState(false);
  const [closing, setClosing] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    api
      .kwic({ keywords: [word.lemma], field: "lemma", pos: word.pos, personIds: performerIds, limit: KWIC_LIMIT })
      .then((hits) => {
        if (!alive) return;
        setSongs(groupHitsBySong(hits));
        setTruncated(hits.length >= KWIC_LIMIT);
      })
      .catch((e) => {
        if (!alive) return;
        onError(message(e));
        setSongs([]);
      });
    return () => {
      alive = false;
    };
  }, [word, performerIds, onError]);

  // 放在点的位置旁边，出不去窗口；变形原点对准点的地方，看起来是从词里长出来的
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    let left = x + 18;
    if (left + r.width > window.innerWidth - 12) left = x - r.width - 18;
    left = Math.max(12, left);
    const top = Math.max(12, Math.min(y - 40, window.innerHeight - r.height - 12));
    setPos((old) => (old && old.left === left && old.top === top ? old : { left, top }));
  }, [x, y, songs]);

  const close = useCallback(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return onClose();
    setClosing(true);
    window.setTimeout(onClose, 180);
  }, [onClose]);

  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      const target = e.target as HTMLElement;
      if (ref.current?.contains(target) || target.closest(".wc-word")) return;
      close();
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [close]);

  const photos = useMemo(() => new Map(people.map((p) => [p.name, p.imagePath])), [people]);
  // 合作署名「A/B」退到第一位演唱者的照片，和曲库网格一样
  const photoOf = (artist: string) => photos.get(artist) ?? photos.get(artist.split("/")[0]?.trim() ?? "") ?? "";
  const byArtist = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of songs ?? []) counts.set(s.artist, (counts.get(s.artist) ?? 0) + s.count);
    return [...counts].sort((a, b) => b[1] - a[1]).slice(0, 8);
  }, [songs]);
  const total = songs?.reduce((n, s) => n + s.count, 0) ?? 0;

  return createPortal(
    <div
      ref={ref}
      className={`wc-pop${closing ? " closing" : ""}`}
      role="dialog"
      aria-label={`「${word.lemma}」出现的歌`}
      style={{
        left: pos?.left ?? x,
        top: pos?.top ?? y,
        visibility: pos ? "visible" : "hidden",
        ["--ox" as string]: `${x - (pos?.left ?? x)}px`,
        ["--oy" as string]: `${y - (pos?.top ?? y)}px`,
        ["--hl" as string]: highlight,
      }}
    >
      <div className="wc-pop-head">
        <span className="wc-pop-ghost" style={{ fontFamily: stack }} aria-hidden="true">
          {word.lemma}
        </span>
        <b style={{ fontFamily: stack }} lang="ja">
          {word.lemma}
        </b>
        <span>
          {songs === null
            ? "正在查…"
            : `出现 ${pyGrouped(total)} 次 · ${songs.length} 首歌${truncated ? `（只查了前 ${pyGrouped(KWIC_LIMIT)} 行）` : ""}`}
        </span>
        <button className="wc-pop-close" onClick={close} aria-label="关闭">
          ✕
        </button>
      </div>
      {byArtist.length > 1 && (
        <div className="wc-pop-by">
          {byArtist.map(([artist, n]) => (
            <span key={artist}>
              <Avatar path={photoOf(artist)} name={artist} size={18} />
              {artist} {pyGrouped(n)}
            </span>
          ))}
        </div>
      )}
      <div className="wc-pop-list">
        {songs !== null && songs.length === 0 && <p className="muted small wc-pop-none">没查到例句</p>}
        {songs?.map((s, i) => (
          <button
            key={s.songId}
            className="wc-song"
            style={{ animationDelay: `${60 + Math.min(i, 12) * 35}ms` }}
            onClick={() => onOpenSong(s.songId, s.first.utteranceId, s.first.timeSec)}
            title={s.first.text}
          >
            <Avatar path={photoOf(s.artist)} name={s.artist} size={40} />
            <span className="wc-song-meta">
              <strong>{s.title}</strong>
              <small>{s.artist}</small>
              <em lang="ja" style={{ fontFamily: stack }}>
                {s.first.left}
                <mark>{s.first.keyword}</mark>
                {s.first.right}
              </em>
            </span>
            <span className="wc-song-side">
              {s.count} 次
              <span className="wc-go">{s.first.timeSec !== null ? `${formatTime(s.first.timeSec)} →` : "→"}</span>
            </span>
          </button>
        ))}
      </div>
      <div className="wc-pop-foot">
        <button onClick={() => onSearchWord(word.lemma)}>在检索页看 KWIC</button>
        <button onClick={() => onShowExamples(word.lemma)}>曲库里看例句</button>
      </div>
    </div>,
    document.body,
  );
}

function formatTime(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/* ── 导出 PNG：在 canvas 里照着屏幕上的样子重画一遍，不靠截图 ── */
async function loadBitmap(path: string): Promise<ImageBitmap | null> {
  try {
    // fetch 而不是 <img>：asset 协议带 CORS 头，canvas 不会被污染，toBlob 才导得出来
    const response = await fetch(convertFileSrc(path));
    if (!response.ok) return null;
    return await createImageBitmap(await response.blob());
  } catch {
    return null;
  }
}

function drawCover(ctx: CanvasRenderingContext2D, img: ImageBitmap, x: number, y: number, w: number, h: number) {
  const scale = Math.max(w / img.width, h / img.height);
  const sw = w / scale;
  const sh = h / scale;
  ctx.drawImage(img, (img.width - sw) / 2, (img.height - sh) / 2, sw, sh, x, y, w, h);
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
}

async function renderPng(opts: {
  placed: Placed[];
  width: number;
  height: number;
  stack: string;
  tint: string;
  title: string;
  photos: string[];
}): Promise<Uint8Array> {
  const { placed, width, height, stack, tint, title, photos } = opts;
  const SCALE = 2;
  const PAD = 28;
  const HEAD = 64;
  const W = width + PAD * 2;
  const H = height + PAD * 2 + HEAD;
  const canvas = document.createElement("canvas");
  canvas.width = W * SCALE;
  canvas.height = H * SCALE;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("画布不可用");
  ctx.scale(SCALE, SCALE);

  // 底图：照片放大、糊开、提饱和，和屏幕上一样
  ctx.fillStyle = "#121218";
  ctx.fillRect(0, 0, W, H);
  const bitmaps = (await Promise.all(photos.map(loadBitmap))).filter((b): b is ImageBitmap => b !== null);
  ctx.save();
  ctx.filter = "blur(48px) saturate(1.5)";
  const ox = -W * 0.18;
  const oy = -H * 0.18;
  const bw = W * 1.36;
  const bh = H * 1.36;
  const [only] = bitmaps;
  if (bitmaps.length === 1 && only) drawCover(ctx, only, ox, oy, bw, bh);
  bitmaps.forEach((b, i) => {
    if (bitmaps.length === 1) return;
    const spot = mosaicSpot(i, bitmaps.length);
    const pct = (v: unknown) => parseFloat(String(v)) / 100;
    drawCover(ctx, b, ox + pct(spot.left) * bw, oy + pct(spot.top) * bh, bw * 0.55, bh * 0.55);
  });
  ctx.restore();
  bitmaps.forEach((b) => b.close());
  const dim = ctx.createRadialGradient(W / 2, H * 0.3, 0, W / 2, H * 0.3, Math.max(W, H) * 0.75);
  dim.addColorStop(0, "rgba(10,10,16,.15)");
  dim.addColorStop(1, "rgba(10,10,16,.65)");
  ctx.fillStyle = dim;
  ctx.fillRect(0, 0, W, H);

  // 标题
  ctx.fillStyle = "#fff";
  ctx.font = `600 22px "Segoe UI", "Yu Gothic UI", "Microsoft YaHei UI", sans-serif`;
  ctx.textBaseline = "middle";
  ctx.fillText(title, PAD, PAD + 18);
  ctx.fillStyle = "rgba(255,255,255,.6)";
  ctx.font = `12px "Segoe UI", "Yu Gothic UI", "Microsoft YaHei UI", sans-serif`;
  ctx.textAlign = "right";
  ctx.fillText("JPOP Corpus Tool", W - PAD, PAD + 18);
  ctx.textAlign = "left";

  // 玻璃
  const gx = PAD - 10;
  const gy = PAD + HEAD - 10;
  const glass = ctx.createLinearGradient(gx, gy, gx + width, gy + height);
  glass.addColorStop(0, "rgba(255,255,255,.14)");
  glass.addColorStop(0.45, "rgba(255,255,255,.04)");
  glass.addColorStop(1, "rgba(255,255,255,.07)");
  roundRect(ctx, gx, gy, width + 20, height + 20, 12);
  ctx.fillStyle = glass;
  ctx.fill();
  ctx.strokeStyle = "rgba(255,255,255,.18)";
  ctx.stroke();

  // 词
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  for (const p of placed) {
    const c = wordColor(p.rank, placed.length, tint);
    ctx.fillStyle = c.color;
    ctx.shadowColor = p.rank < 6 ? withAlpha(tint, 0.45) : "rgba(0,0,0,.35)";
    ctx.shadowBlur = p.rank < 6 ? 20 : 8;
    ctx.font = `${p.fontSize}px ${stack}`;
    const x = PAD + p.x + p.w / 2;
    const y = PAD + HEAD + p.y;
    if (p.vertical) {
      [...p.word.lemma].forEach((ch, k) => ctx.fillText(ch, x, y + p.fontSize * 0.06 + p.fontSize * (k + 0.5)));
    } else {
      ctx.fillText(p.word.lemma, x, y + p.h / 2);
    }
  }

  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("生成图片失败");
  return new Uint8Array(await blob.arrayBuffer());
}
