/**
 * 关系网的画布（照 Obsidian 的图谱）：节点会动、能拖、滚轮缩放、拖空白处平移；
 * 鼠标移到一个人上，他和他的邻居亮着、其余的淡下去；放大到一定程度才显示所有名字。
 *
 * 用 canvas 不用 SVG：几百个节点每帧重画，DOM 会卡。模拟在 `graph.ts`。
 * 系统开了「减少动态效果」时一口气摆好、不做动画。
 */

import { useCallback, useEffect, useRef } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { nodeAt, Simulation, type Graph, type GraphNode } from "../graph";
import { prefersReducedMotion } from "../motion";
import { ROLE_COLORS } from "./LibraryBrowser";
import { monogram } from "../avatarIdentity";
import { tr } from "../i18n";

interface Props {
  graph: Graph;
  /** 局部图的中心 / 全局图里选中的人：一直亮着、带一圈光 */
  focusId?: number | null;
  /** 搜索词：名字对得上的亮着 */
  query?: string;
  /** 小图（右栏）：名字全显示，不响应滚轮缩放（免得滚右栏时被它吃掉） */
  compact?: boolean;
  onSelect?: (id: number) => void;
  /** 双击：打开这个人 */
  onOpen?: (id: number) => void;
  className?: string;
  /** 改了这个就把视野重新对准全图 */
  fitKey?: string | number;
}

interface View {
  x: number;
  y: number;
  k: number;
}

const DIM = 0.12;

export function RelationGraph({ graph, focusId = null, query = "", compact = false, onSelect, onOpen, className, fitKey }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const state = useRef({
    sim: new Simulation(graph),
    view: { x: 0, y: 0, k: 1 } as View,
    size: { w: 0, h: 0, dpr: 1 },
    hover: -1,
    drag: null as null | { kind: "node"; i: number; moved: number } | { kind: "pan"; sx: number; sy: number; vx: number; vy: number; moved: number },
    vis: new Float32Array(graph.nodes.length).fill(0),
    appear: new Float32Array(graph.nodes.length).fill(0),
    born: 0,
    userMoved: false,
    frame: 0,
    images: new Map<string, HTMLImageElement | null>(),
  });
  const props = useRef({ focusId, query, compact, onSelect, onOpen });
  props.current = { focusId, query, compact, onSelect, onOpen };

  // 每帧画的那个函数每次渲染都换成最新的（里面读的是这一次的 graph），kick 永远调最新的
  const loopRef = useRef<() => void>(() => undefined);
  const kick = useCallback(() => {
    const st = state.current;
    if (st.frame === 0) st.frame = requestAnimationFrame(() => loopRef.current());
  }, []);

  /** 让全图落在画布里（留边） */
  const fit = useCallback(() => {
    const st = state.current;
    const { nodes } = graph;
    if (nodes.length === 0 || st.size.w === 0) return;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of nodes) {
      minX = Math.min(minX, n.x - n.r);
      minY = Math.min(minY, n.y - n.r);
      maxX = Math.max(maxX, n.x + n.r);
      maxY = Math.max(maxY, n.y + n.r);
    }
    const pad = compact ? 28 : 60;
    const inset = compact ? 0 : Math.min(260, st.size.w * 0.26);
    const k = Math.min(compact ? 2 : 3, Math.max(0.15, Math.min((st.size.w - inset - pad * 2) / (maxX - minX || 1), (st.size.h - pad * 2) / (maxY - minY || 1))));
    st.view = { k, x: (st.size.w + inset) / 2 - ((minX + maxX) / 2) * k, y: st.size.h / 2 - ((minY + maxY) / 2) * k };
  }, [graph, compact]);

  // 换图：新的模拟，从头长出来
  useEffect(() => {
    const st = state.current;
    st.sim = new Simulation(graph);
    st.vis = new Float32Array(graph.nodes.length).fill(0);
    st.appear = new Float32Array(graph.nodes.length).fill(0);
    st.hover = -1;
    st.userMoved = false;
    st.born = performance.now();
    if (prefersReducedMotion()) {
      st.sim.settle();
      st.vis.fill(1);
      st.appear.fill(1);
      fit();
    }
    kick();
  }, [graph, fit, kick]);

  useEffect(() => {
    state.current.userMoved = false;
    fit();
    kick();
  }, [fitKey, fit, kick]);

  // 焦点、搜索词变了要重画
  useEffect(kick, [focusId, query, kick]);

  // 画布尺寸跟着容器走
  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas === null) return;
    const ro = new ResizeObserver(() => {
      const rect = canvas.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      const st = state.current;
      st.size = { w: rect.width, h: rect.height, dpr };
      canvas.width = Math.round(rect.width * dpr);
      canvas.height = Math.round(rect.height * dpr);
      if (!st.userMoved) fit();
      kick();
    });
    ro.observe(canvas);
    return () => ro.disconnect();
  }, [fit, kick]);

  useEffect(
    () => () => {
      const st = state.current;
      if (st.frame !== 0) cancelAnimationFrame(st.frame);
      st.frame = 0;
    },
    [],
  );

  function image(path: string): HTMLImageElement | null {
    const st = state.current;
    if (!path) return null;
    if (!st.images.has(path)) {
      const img = new Image();
      st.images.set(path, null);
      img.onload = () => {
        st.images.set(path, img);
        kick();
      };
      img.src = convertFileSrc(path);
    }
    return st.images.get(path) ?? null;
  }

  function loop() {
    const st = state.current;
    st.frame = 0;
    const canvas = canvasRef.current;
    if (canvas === null) return;
    const ctx = canvas.getContext("2d");
    if (ctx === null) return;
    const { nodes, links, neighbors } = graph;
    const { focusId, query } = props.current;

    if (st.sim.running) {
      st.sim.tick();
      // 刚出来的头一秒跟着图的大小对准视野；用户动过视野就不再管
      if (!st.userMoved && performance.now() - st.born < 1400) fit();
    }

    // 谁亮着：悬停的人和他的邻居；没悬停时看搜索词；都没有就全亮
    const q = query.trim().toLowerCase();
    const lit = new Set<number>();
    if (st.hover >= 0) {
      lit.add(st.hover);
      for (const n of neighbors[st.hover] ?? []) lit.add(n);
    } else if (q !== "") {
      nodes.forEach((n, i) => n.name.toLowerCase().includes(q) && lit.add(i));
    }
    const anyLit = lit.size > 0;
    let fading = false;
    const now = performance.now();
    nodes.forEach((n, i) => {
      // 出场：按离中心的远近依次长出来
      const t = prefersReducedMotion() ? 1 : Math.min(1, Math.max(0, (now - st.born - n.depth * 160 - (i % 40) * 8) / 420));
      // 带一点回弹地长出来
      const appear = t >= 1 ? 1 : 1 - Math.pow(1 - t, 3) * Math.cos(t * Math.PI * 1.5);
      st.appear[i] = appear;
      const target = (anyLit && !lit.has(i) ? DIM : 1) * Math.min(1, t * 1.5);
      const v = st.vis[i]!;
      const next = v + (target - v) * 0.2;
      st.vis[i] = Math.abs(next - target) < 0.01 ? target : next;
      if (st.vis[i] !== target || t < 1) fading = true;
    });

    const { w, h, dpr } = st.size;
    const { x: tx, y: ty, k } = st.view;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.setTransform(dpr * k, 0, 0, dpr * k, dpr * tx, dpr * ty);

    const theme = getComputedStyle(canvas);
    const tone = (name: string) => theme.getPropertyValue(name).trim();
    const fg = tone("--fg"), muted = tone("--muted"), surface = tone("--surface-pop"), accent = tone("--accent"), line = tone("--line-strong");
    // Edges end at the portrait rim, with a single restrained accent on hover.
    for (const l of links) {
      const s = nodes[l.s]!;
      const t = nodes[l.t]!;
      const hot = st.hover >= 0 && (l.s === st.hover || l.t === st.hover);
      const alpha = Math.min(st.vis[l.s]!, st.vis[l.t]!);
      ctx.globalAlpha = hot ? 0.7 : 0.28 * alpha;
      ctx.strokeStyle = hot ? accent : muted;
      ctx.lineWidth = (hot ? 1.3 : 0.7) / k;
      ctx.setLineDash(l.member ? [4 / k, 4 / k] : []);
      ctx.beginPath();
      const distance = Math.hypot(t.x-s.x,t.y-s.y) || 1;
      const dx=(t.x-s.x)/distance, dy=(t.y-s.y)/distance;
      ctx.moveTo(s.x+dx*Math.min(s.r,21/k),s.y+dy*Math.min(s.r,21/k));
      ctx.lineTo(t.x-dx*Math.min(t.r,21/k),t.y-dy*Math.min(t.r,21/k));
      ctx.stroke();
    }
    ctx.setLineDash([]);

    // 点
    nodes.forEach((n, i) => {
      const v = st.vis[i]!;
      if (v <= 0.001) return;
      const focus = n.id === focusId;
      const hovered = i === st.hover;
      const r = Math.min(n.r, 21 / k) * Math.max(0.05, st.appear[i]!) * (hovered ? 1.08 : 1);
      ctx.globalAlpha = Math.max(v, 0.06);
      if (focus || hovered) {
        ctx.strokeStyle = accent;
        ctx.lineWidth = 1.5 / k;
        ctx.beginPath(); ctx.arc(n.x, n.y, r + 3 / k, 0, Math.PI * 2); ctx.stroke();
      }
      const img = r * k > 9 ? image(n.imagePath) : null;
      ctx.beginPath();
      ctx.arc(n.x, n.y, r, 0, Math.PI * 2);
      if (img !== null) {
        ctx.save();
        ctx.clip();
        const scale = Math.max(2*r/img.naturalWidth, 2*r/img.naturalHeight);
        ctx.drawImage(img, n.x-img.naturalWidth*scale/2, n.y-img.naturalHeight*scale/2, img.naturalWidth*scale, img.naturalHeight*scale);
        ctx.restore();
      } else {
        const fill = ctx.createLinearGradient(n.x-r,n.y-r,n.x+r,n.y+r);
        fill.addColorStop(0, surface); fill.addColorStop(1, line);
        ctx.fillStyle = fill; ctx.fill();
        if (r*k > 10) {
          ctx.fillStyle = fg; ctx.textAlign = "center"; ctx.textBaseline = "middle";
          ctx.font = `500 ${r * (monogram(n.name).length > 2 ? .62 : .8)}px "Segoe UI", "Yu Gothic UI", sans-serif`;
          ctx.fillText(monogram(n.name),n.x,n.y,r*1.7);
        }
      }
      ctx.beginPath(); ctx.arc(n.x,n.y,r,0,Math.PI*2);
      ctx.lineWidth = 1 / k; ctx.strokeStyle = n.isBand ? fg : line; ctx.stroke();
      if (n.r*k > 10) {
        ctx.beginPath(); ctx.arc(n.x+r*.7,n.y+r*.7,2.5/k,0,Math.PI*2);
        ctx.fillStyle=ROLE_COLORS[n.role] ?? muted; ctx.fill();
      }
    });

    // Place selected/hovered names first, then omit colliding secondary labels.
    const labels: {left:number;top:number;right:number;bottom:number}[]=[];
    const fontPx=12/k;
    ctx.font=`500 ${fontPx}px "Segoe UI Variable Text", "Yu Gothic UI", "Microsoft YaHei UI", sans-serif`;
    ctx.textAlign="center"; ctx.textBaseline="top";
    const order=nodes.map((n,i)=>({n,i})).sort((a,b)=>Number(b.n.id===focusId || b.i===st.hover)-Number(a.n.id===focusId || a.i===st.hover) || b.n.songs-a.n.songs);
    for (const {n,i} of order) {
      const v=st.vis[i]!;
      const priority=n.id===focusId || i===st.hover;
      if (v<.05 || (!priority && n.r*k<10 && k<1.6)) continue;
      let name=n.name;
      while (ctx.measureText(name).width*k>144 && [...name].length>2) name=[...name].slice(0,-2).join("")+"…";
      const width=ctx.measureText(name).width*k+12;
      const cx=Math.max(width/2+4,Math.min(w-width/2-4,n.x*k+tx));
      const top=(n.y+Math.min(n.r,21/k))*k+ty+7;
      const box={left:cx-width/2,top,right:cx+width/2,bottom:top+20};
      if (top<0 || box.bottom>h || labels.some(b=>box.left<b.right && box.right>b.left && box.top<b.bottom && box.bottom>b.top)) continue;
      if (!priority && nodes.some(other => other !== n && (() => { const r=Math.min(other.r*k,21)+3, x=other.x*k+tx, y=other.y*k+ty; return box.left<x+r && box.right>x-r && box.top<y+r && box.bottom>y-r; })())) continue;
      labels.push(box);
      const x=(cx-tx)/k, y=(top-ty)/k;
      ctx.globalAlpha=priority ? 1 : v*.9;
      ctx.fillStyle=surface;
      ctx.beginPath(); ctx.roundRect(x-width/(2*k),y-2/k,width/k,20/k,4/k); ctx.fill();
      ctx.fillStyle=priority ? fg : muted; ctx.fillText(name,x,y);
    }
    ctx.globalAlpha = 1;

    if (st.sim.running || fading || st.drag !== null) st.frame = requestAnimationFrame(loop);
  }

  loopRef.current = loop;

  // ── 指针 ──
  const toGraph = (e: { clientX: number; clientY: number }) => {
    const rect = canvasRef.current!.getBoundingClientRect();
    const { x, y, k } = state.current.view;
    return { x: (e.clientX - rect.left - x) / k, y: (e.clientY - rect.top - y) / k };
  };

  const onPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const st = state.current;
    const p = toGraph(e);
    const i = nodeAt(graph.nodes, p.x, p.y, 4 / st.view.k, 21 / st.view.k);
    e.currentTarget.setPointerCapture(e.pointerId);
    if (i >= 0) {
      st.drag = { kind: "node", i, moved: 0 };
      const n = graph.nodes[i]!;
      n.fx = n.x;
      n.fy = n.y;
      st.sim.reheat(0.3);
    } else {
      st.drag = { kind: "pan", sx: e.clientX, sy: e.clientY, vx: st.view.x, vy: st.view.y, moved: 0 };
    }
    kick();
  };

  const onPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const st = state.current;
    const d = st.drag;
    if (d?.kind === "node") {
      const p = toGraph(e);
      const n = graph.nodes[d.i]!;
      d.moved += Math.hypot(e.movementX, e.movementY);
      n.fx = p.x;
      n.fy = p.y;
      st.sim.reheat(0.25);
      kick();
      return;
    }
    if (d?.kind === "pan") {
      d.moved += Math.hypot(e.movementX, e.movementY);
      st.view = { ...st.view, x: d.vx + e.clientX - d.sx, y: d.vy + e.clientY - d.sy };
      st.userMoved = true;
      kick();
      return;
    }
    const p = toGraph(e);
    const i = nodeAt(graph.nodes, p.x, p.y, 4 / st.view.k, 21 / st.view.k);
    if (i !== st.hover) {
      st.hover = i;
      e.currentTarget.style.cursor = i >= 0 ? "pointer" : "grab";
      kick();
    }
  };

  const onPointerUp = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const st = state.current;
    const d = st.drag;
    st.drag = null;
    if (d?.kind === "node") {
      const n = graph.nodes[d.i]!;
      // 拖过的放手后留在原地（Obsidian 也是），只是点一下的不钉
      if (d.moved < 4) {
        n.fx = null;
        n.fy = null;
        props.current.onSelect?.(n.id);
      } else {
        n.fx = null;
        n.fy = null;
        st.sim.reheat(0.2);
      }
    }
    e.currentTarget.releasePointerCapture(e.pointerId);
    kick();
  };

  const onLeave = () => {
    const st = state.current;
    if (st.hover !== -1 && st.drag === null) {
      st.hover = -1;
      kick();
    }
  };

  const onDoubleClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const p = toGraph(e);
    const i = nodeAt(graph.nodes, p.x, p.y, 4 / state.current.view.k, 21 / state.current.view.k);
    if (i >= 0) props.current.onOpen?.(graph.nodes[i]!.id);
  };

  // 滚轮缩放（以指针为中心）。要 passive: false 才能拦住页面滚动，所以手动挂
  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas === null || compact) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const st = state.current;
      const rect = canvas.getBoundingClientRect();
      const px = e.clientX - rect.left;
      const py = e.clientY - rect.top;
      const k = Math.min(5, Math.max(0.12, st.view.k * Math.exp(-e.deltaY * 0.0015)));
      st.view = { k, x: px - ((px - st.view.x) / st.view.k) * k, y: py - ((py - st.view.y) / st.view.k) * k };
      st.userMoved = true;
      kick();
    };
    canvas.addEventListener("wheel", onWheel, { passive: false });
    return () => canvas.removeEventListener("wheel", onWheel);
  }, [compact, kick]);

  return (
    <canvas
      ref={canvasRef}
      className={`relation-graph ${className ?? ""}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onPointerLeave={onLeave}
      onDoubleClick={onDoubleClick}
      aria-label={tr("关系网：拖动节点、拖空白处平移、滚轮缩放，双击打开这个人")}
    />
  );
}

export type { GraphNode };
