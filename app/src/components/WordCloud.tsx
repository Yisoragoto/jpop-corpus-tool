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

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

import { api, POS_LABELS, type FrequencyRow, type PersonSummary } from "../api";
import { imageColor, toHighlight } from "../coverColor";
import { ensureFontFile, fetchFontCatalog, loadImportedFonts, lyricFontStack } from "../fonts";
import { pyGrouped } from "../pyFormat";
import { holdFullscreen, releaseFullscreen } from "../fullscreen";
import { useAppSettings } from "../settings";
import {
  filterCloudRows,
  layoutCloud,
  mergeByLemma,
  OTHER_POS_COLOR,
  POS_COLORS,
  variantKey,
  type CloudWord,
  type LemmaForm,
  type Placed,
  type WordJump,
} from "../wordCloud";
import { Avatar } from "./Avatar";
import { WordWhere } from "./WordWhere";
import { loadPerformers } from "./PerformerPicker";
import { tr } from "../i18n";

/** 底图最多拼几位歌手的照片 */
const BACKDROP_PEOPLE = 5;
/** 没有照片可取色时的高光色：界面的强调色 */
const DEFAULT_TINT = "rgb(159, 216, 255)";

export interface NowLine {
  songId: string;
  title: string;
  artist: string;
  text: string;
}

interface Props {
  rows: FrequencyRow[] | null;
  /** 筛选的演唱者，空 = 全部 */
  performerIds: number[];
  /** 发行年份范围（含两端），点词查歌时也按它筛。null = 不限 */
  years: { from: number | null; to: number | null };
  loading: boolean;
  /** 卡片头右边的工具（筛选、字体……） */
  toolbar?: ReactNode;
  /** 选中的词（右边列出它出现在哪些歌）。由外面记着：跳去看歌词再回来还在 */
  pickedLemma: string | null;
  onPick: (lemma: string | null) => void;
  /** 正在放的那一处（列表里那一行亮着） */
  playing: { songId: string; utteranceId: number } | null;
  /** 正在放的那一句：在云下面当字幕显示，选中的词标亮 */
  nowLine: NowLine | null;
  /** 从这一句开始放。`fullscreen` = 同时进全屏歌词（退出来还是词云）；否则只在词云里放 */
  onPlayLine: (songId: string, utteranceId: number, timeSec: number | null, fullscreen: boolean) => void;
  /** 去检索页按词元查，范围和词云一样 */
  onSearchWord: (lemma: string, jump: WordJump) => void;
  /** 去看这个词的释义和例句，范围和词云一样 */
  onShowExamples: (lemma: string, jump: WordJump) => void;
  /** 全屏：窗口全屏显示词云 */
  present: boolean;
  /** 上面盖着全屏歌词：词云这边不响应 Esc，也退到播放条底下 */
  covered: boolean;
  /** 一首歌的封面（右边列表里显示歌的封面，不是歌手头像） */
  coverOf: (songId: string) => string;
  onPresent: (on: boolean) => void;
  onError: (message: string) => void;
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

/**
 * 名次 → 颜色。默认前几名染上照片的颜色，其余白色按名次渐淡；
 * 按词性上色时（`pos` 给了颜色）每种词性一种颜色，同样按名次渐淡、前几名带光晕。
 */
function wordColor(rank: number, total: number, tint: string, pos: string | null = null): { color: string; shadow: string } {
  const alpha = 0.5 + 0.5 * Math.pow(1 - rank / Math.max(1, total), 1.4);
  if (pos !== null) {
    if (rank < 6) return { color: pos, shadow: `0 0 24px ${withAlpha(pos, 0.4)}, 0 2px 10px rgba(0,0,0,.35)` };
    return { color: withAlpha(pos, Number(Math.max(0.62, alpha).toFixed(2))), shadow: "0 1px 8px rgba(0,0,0,.35)" };
  }
  if (rank < 6) return { color: tint, shadow: `0 0 24px ${withAlpha(tint, 0.4)}, 0 2px 10px rgba(0,0,0,.35)` };
  return { color: `rgba(255,255,255,${alpha.toFixed(2)})`, shadow: "0 1px 8px rgba(0,0,0,.35)" };
}

function posColor(word: CloudWord): string {
  return POS_COLORS[word.mainPos] ?? OTHER_POS_COLOR;
}

/* ── 异体写法：词元的正规化写法和读音，查过的记在模块里，换筛选不用再问 ── */
const formCache = new Map<string, LemmaForm>();

function useLemmaForms(rows: FrequencyRow[] | null): number {
  const [version, setVersion] = useState(0);
  useEffect(() => {
    if (rows === null) return;
    const missing = [...new Set(rows.map((r) => r.lemma))].filter((l) => !formCache.has(l));
    if (missing.length === 0) return;
    let alive = true;
    api
      .lemmaForms(missing)
      .then((forms) => {
        for (const f of forms) formCache.set(f.lemma, f);
        if (alive && forms.length > 0) setVersion((v) => v + 1);
      })
      // 没装分词词典、后端出错：不合并就是了，云照样画
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [rows]);
  return version;
}

function withAlpha(rgb: string, alpha: number): string {
  return rgb.replace(/^rgb\((.+)\)$/, `rgba($1, ${alpha})`);
}

/* ── 字体：选的字体有文件就先加载，加载完再排布（量字宽要用真字体） ── */
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
export type { WordJump };

export function WordCloud({
  rows,
  performerIds,
  years,
  loading,
  toolbar,
  pickedLemma,
  onPick,
  playing,
  nowLine,
  onPlayLine,
  onSearchWord,
  onShowExamples,
  present,
  covered,
  coverOf,
  onPresent,
  onError,
}: Props) {
  const settings = useAppSettings();
  const { stack, ready } = useCloudFont(settings.wordCloudFont);
  const [people, setPeople] = useState<PersonSummary[]>([]);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [tint, setTint] = useState(DEFAULT_TINT);
  const [saving, setSaving] = useState(false);

  const stageRef = useRef<HTMLDivElement>(null);
  const glassRef = useRef<HTMLDivElement>(null);
  const cloudRef = useRef<HTMLDivElement>(null);
  const bgRef = useRef<HTMLDivElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const motion = useMemo(() => new CloudMotion(), []);
  const shrinkRef = useRef(1);
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

  // 全屏 = 窗口真正全屏（盖住任务栏）。离开词云时一并退出
  useEffect(() => {
    if (!present) return;
    holdFullscreen("cloud");
    return () => releaseFullscreen("cloud");
  }, [present]);

  // 全屏时按 Esc 退出（上面盖着全屏歌词时，Esc 归歌词）
  useEffect(() => {
    if (!present || covered) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && pickedLemma === null) onPresent(false);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [present, covered, pickedLemma, onPresent]);

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

  // 云跟着那块玻璃的大小排。差几十像素不重排，免得拖窗口时闪
  useLayoutEffect(() => {
    const el = cloudRef.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const w = Math.round(entry.contentRect.width);
      const h = Math.round(entry.contentRect.height);
      setSize((old) => (Math.abs(old.w - w) >= 24 || Math.abs(old.h - h) >= 24 ? { w, h } : old));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const count = settings.wordCloudCount;
  const hideFunctionWords = settings.wordCloudHideFunctionWords;
  const hidden = settings.wordCloudHidden;
  const formsVersion = useLemmaForms(rows);
  const colorByPos = settings.wordCloudColorByPos;
  const words = useMemo(
    () =>
      mergeByLemma(filterCloudRows(rows ?? [], hideFunctionWords, new Set(hidden)), count, (lemma) =>
        variantKey(lemma, formCache.get(lemma)),
      ),
    // formsVersion：后端的词形回来了要重新合并
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rows, count, hideFunctionWords, hidden, formsVersion],
  );
  // 图例：云里实际有哪些词性、各几个
  const legend = useMemo(() => {
    const n = new Map<string, number>();
    for (const w of words) n.set(w.mainPos, (n.get(w.mainPos) ?? 0) + 1);
    return [...n].sort((a, b) => b[1] - a[1]);
  }, [words]);
  const placed = useMemo(() => {
    if (size.w === 0 || size.h < 120 || ready === 0) return [];
    const ctx = document.createElement("canvas").getContext("2d");
    const measure = (text: string, fontSize: number) => {
      if (!ctx) return [...text].length * fontSize;
      ctx.font = `${fontSize}px ${stack}`;
      return ctx.measureText(text).width;
    };
    // 字号跟着画面大小走：全屏时整片放大，不是只多出一圈空白
    const scale = Math.sqrt((size.w * size.h) / (900 * 440));
    return layoutCloud(words, {
      width: size.w,
      height: size.h,
      minFont: Math.round(Math.min(20, Math.max(12, 13 * scale))),
      maxFont: Math.min(Math.round(84 * scale), size.w / 8, size.h / 3),
      verticalRatio: 0.22,
      measure,
    });
  }, [words, size, stack, ready]);

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
    motion.pointer = { x: (e.clientX - cr.left) / shrinkRef.current, y: (e.clientY - cr.top) / shrinkRef.current };
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
    const spellings = p.word.variants.length > 1 ? `${tr("（")}${p.word.variants.join("・")}${tr("）")}` : "";
    tip.textContent = tr("{lemma}{spellings} · {v} · {freq} 次 · {songCount} 首", { lemma: p.word.lemma, spellings, v: POS_LABELS[p.word.mainPos] ?? p.word.mainPos, freq: pyGrouped(p.word.freq), songCount: pyGrouped(p.word.songCount) });
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

  const click = (i: number, p: Placed) => {
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
    onPick(pickedLemma === p.word.lemma ? null : p.word.lemma);
  };

  const pickedWord =
    pickedLemma === null ? null : (words.find((w) => w.lemma === pickedLemma || w.variants.includes(pickedLemma)) ?? null);
  // 右边那一栏出来时，云整片缩小让出位置（不重排：重排的话每个词都会跳一下）
  const panelWidth = Math.min(340, size.w * 0.36);
  const shrink = pickedWord !== null && size.w > 0 ? Math.max(0.5, (size.w - panelWidth) / size.w) : 1;
  shrinkRef.current = shrink;
  const pickedRank = pickedWord === null ? -1 : words.indexOf(pickedWord);

  const who =
    chosen.length === 0 ? tr("全部歌手") : chosen.length <= 3 ? chosen.map((p) => p.name).join(tr("、")) : tr("{map} 等 {length} 位", { map: chosen.slice(0, 3).map((p) => p.name).join(tr("、")), length: chosen.length });
  const songTotal = chosen.length === 0 ? null : chosen.reduce((n, p) => n + p.trackCount, 0);

  const savePng = async () => {
    setSaving(true);
    try {
      const path = await save({
        title: tr("保存词云"),
        defaultPath: "wordcloud.png",
        filters: [{ name: tr("PNG 图片"), extensions: ["png"] }],
      });
      if (!path) return;
      const bytes = await renderPng({ placed, width: size.w, height: size.h, stack, tint, title: who, photos: backdrop.map((p) => p.imagePath), colorByPos });
      await api.exportPng(path, bytes);
    } catch (err) {
      onError(tr("词云没能保存：{err}", { err: message(err) }));
    } finally {
      setSaving(false);
    }
  };

  const empty = rows !== null && words.length === 0;

  return (
    <section
      className={`wc-stage${present ? " present" : ""}${pickedWord ? " with-panel" : ""}${covered ? " covered" : ""}`}
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
          {chosen.length === 0 && backdrop.length === 0 && <span className="avatar avatar-text wc-all">{tr("全")}</span>}
          {(chosen.length > 0 ? chosen : backdrop).slice(0, 3).map((p) => (
            <Avatar key={p.id} path={p.imagePath} name={p.name} size={chosen.length === 1 ? 56 : 40} />
          ))}
        </div>
        <div className="wc-who">
          <h2>{who}</h2>
          <span>
            {songTotal !== null && tr("{songTotal} 首 · ", { songTotal: pyGrouped(songTotal) })}
            {loading ? tr("读取中…") : tr("前 {length} 个词", { length: placed.length })}
            {!present && tr(" · 点词看出现在哪些歌")}
          </span>
        </div>
        <div className="wc-tools">
          {!present && (
            <button className="btn wc-btn" disabled={placed.length === 0 || saving} onClick={() => void savePng()}>
              {saving ? tr("保存中…") : tr("保存 PNG")}
            </button>
          )}
          <button className="primary wc-btn" onClick={() => onPresent(!present)} title={present ? tr("退出全屏（Esc）") : tr("全屏显示词云")}>
            {present ? tr("退出全屏") : tr("全屏")}
          </button>
        </div>
      </div>

      {toolbar !== undefined && !present && <div className="wc-bar">{toolbar}</div>}

      <div className="wc-glass" ref={glassRef}>
        <div className={`wc-cloud${loading ? " stale" : ""}${pickedWord ? " has-pick" : ""}`} ref={cloudRef}
          style={{ fontFamily: stack, transform: shrink === 1 ? undefined : `scale(${shrink.toFixed(4)})` }}
          lang="ja"
        >
          {placed.map((p, i) => {
            const c = wordColor(p.rank, words.length, tint, colorByPos ? posColor(p.word) : null);
            return (
              <span
                key={`${p.word.lemma}:${size.w}:${size.h}:${stack}`}
                className={`wc-word${p.vertical ? " v" : ""}${p.word.lemma === pickedLemma ? " chosen" : ""}`}
                style={{
                  left: p.x,
                  top: p.y,
                  width: p.w,
                  height: p.h,
                  ["--glow" as string]: colorByPos ? posColor(p.word) : p.rank < 6 ? tint : "rgba(255,255,255,.55)",
                }}
                role="button"
                tabIndex={0}
                aria-label={tr("{lemma}，{freq} 次，列出出现它的歌", { lemma: p.word.lemma, freq: p.word.freq })}
                onPointerEnter={(e) => enter(i, p, e)}
                onPointerMove={moveTip}
                onPointerLeave={() => leave(i)}
                onClick={() => click(i, p)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    click(i, p);
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
        {empty && <p className="wc-empty">{tr("这组筛选下没有词")}</p>}
        {nowLine !== null && <Caption line={nowLine} word={pickedWord} stack={stack} tint={tint} />}
        {pickedWord !== null && (
          <WordWhere
            key={pickedWord.lemma}
            word={pickedWord}
            highlight={pickedRank >= 0 && pickedRank < 6 ? tint : "rgb(255,255,255)"}
            stack={stack}
            performerIds={performerIds}
            years={years}
            people={people}
            playing={playing}
            compact={present}
            covered={covered}
            coverOf={coverOf}
            onClose={() => onPick(null)}
            onPlayLine={onPlayLine}
            onSearchWord={onSearchWord}
            onShowExamples={onShowExamples}
            onError={onError}
          />
        )}
      </div>
      {colorByPos && legend.length > 0 && (
        <div className="wc-legend" aria-label={tr("词性颜色")}>
          {legend.map(([pos, n]) => (
            <span key={pos}>
              <i style={{ background: POS_COLORS[pos] ?? OTHER_POS_COLOR }} />
              {POS_LABELS[pos] ?? pos}
              <small>{n}</small>
            </span>
          ))}
        </div>
      )}

      <div className="wc-tip" ref={tipRef} hidden />
    </section>
  );
}

/** 正在放的那一句，当字幕摆在云下面。换句时整行淡入，选中的词标亮 */
function Caption({ line, word, stack, tint }: { line: NowLine; word: CloudWord | null; stack: string; tint: string }) {
  const parts = useMemo(() => {
    const keys = word ? [...new Set([word.lemma, ...word.surfaces])].filter((k) => k !== "").sort((a, b) => b.length - a.length) : [];
    if (keys.length === 0) return [{ text: line.text, hit: false }];
    const re = new RegExp(`(${keys.map((k) => k.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`, "g");
    return line.text.split(re).filter((t) => t !== "").map((t) => ({ text: t, hit: keys.includes(t) }));
  }, [line.text, word]);
  return (
    <div className="wc-caption" aria-live="polite">
      <small>
        ♪ {line.title} · {line.artist}
      </small>
      <p key={line.text} lang="ja" style={{ fontFamily: stack }}>
        {parts.map((p, i) =>
          p.hit ? (
            <mark key={i} style={{ color: tint }}>
              {p.text}
            </mark>
          ) : (
            <span key={i}>{p.text}</span>
          ),
        )}
      </p>
    </div>
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
  colorByPos: boolean;
}): Promise<Uint8Array> {
  const { placed, width, height, stack, tint, title, photos, colorByPos } = opts;
  const SCALE = 2;
  const PAD = 28;
  const HEAD = 64;
  const W = width + PAD * 2;
  const H = height + PAD * 2 + HEAD;
  const canvas = document.createElement("canvas");
  canvas.width = W * SCALE;
  canvas.height = H * SCALE;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error(tr("画布不可用"));
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
    const pc = colorByPos ? posColor(p.word) : null;
    const c = wordColor(p.rank, placed.length, tint, pc);
    ctx.fillStyle = c.color;
    ctx.shadowColor = p.rank < 6 ? withAlpha(pc ?? tint, 0.45) : "rgba(0,0,0,.35)";
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
  if (!blob) throw new Error(tr("生成图片失败"));
  return new Uint8Array(await blob.arrayBuffer());
}
