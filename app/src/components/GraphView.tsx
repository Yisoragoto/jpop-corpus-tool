/**
 * 全局关系网：所有人、谁和谁一起署过名。占中栏和右栏。
 *
 * 左上角一块毛玻璃：搜人、按角色筛、「至少一起几首」、只看某人附近。点一个人在左下角出他的卡片，
 * 双击或点卡片上的「打开档案」去他的档案页。乐队和成员之间是虚线。
 */

import { useMemo, useState } from "react";

import { ROLE_LABELS } from "../api";
import { buildGraph } from "../graph";
import { rolesOf } from "../libraryIndex";
import { Avatar } from "./Avatar";
import { ROLE_COLORS, type DirectoryData } from "./LibraryBrowser";
import { RelationGraph } from "./RelationGraph";

const ROLES = ["performer", "composer", "lyricist", "arranger"] as const;

interface Props {
  data: DirectoryData | null;
  initialFocus: number | null;
  onOpenPerson: (id: number) => void;
}

export function GraphView({ data, initialFocus, onOpenPerson }: Props) {
  const [query, setQuery] = useState("");
  const [roles, setRoles] = useState<Set<string>>(() => new Set(ROLES));
  const [minShared, setMinShared] = useState(1);
  const [selected, setSelected] = useState<number | null>(initialFocus);
  // 只看某人附近（两步以内）
  const [around, setAround] = useState<number | null>(null);

  const graph = useMemo(() => {
    if (data === null) return null;
    return buildGraph(data.entries, data.edges, data.memberships, {
      ...(roles.size === ROLES.length ? {} : { roles }),
      minShared,
      ...(around !== null ? { focus: around, depth: 2 } : {}),
    });
  }, [data, roles, minShared, around]);

  const person = selected !== null ? data?.dir.byId.get(selected) : undefined;
  const bands = selected !== null ? (data?.dir.bandsOf.get(selected) ?? []) : [];
  const degree = useMemo(() => {
    if (graph === null || selected === null) return 0;
    const i = graph.nodes.findIndex((n) => n.id === selected);
    return i >= 0 ? graph.neighbors[i]!.length : 0;
  }, [graph, selected]);

  const toggleRole = (r: string) =>
    setRoles((cur) => {
      const next = new Set(cur);
      if (next.has(r)) {
        if (next.size > 1) next.delete(r);
      } else next.add(r);
      return next;
    });

  if (graph === null) return <p className="muted pad">加载中…</p>;

  return (
    <div className="graph-view">
      <RelationGraph
        graph={graph}
        focusId={selected}
        query={query}
        onSelect={setSelected}
        onOpen={onOpenPerson}
        fitKey={`${around ?? "all"}-${minShared}-${[...roles].join()}`}
      />

      <div className="graph-controls glass">
        <input className="search" placeholder="找一个人" value={query} onChange={(e) => setQuery(e.target.value)} />
        <div className="role-chips">
          {ROLES.map((r) => (
            <button key={r} className={`chip ${roles.has(r) ? "on" : ""}`} onClick={() => toggleRole(r)}>
              <i style={{ background: ROLE_COLORS[r] }} />
              {ROLE_LABELS[r] ?? r}
            </button>
          ))}
        </div>
        <label className="graph-slider">
          <span>至少一起 {minShared} 首</span>
          <input type="range" min={1} max={6} value={minShared} onChange={(e) => setMinShared(Number(e.target.value))} />
        </label>
        <p className="muted small graph-tip">
          {graph.nodes.length} 人 · {graph.links.length} 条关系。拖动节点、拖空白处平移、滚轮缩放，双击打开档案
        </p>
      </div>

      <div className="graph-legend glass" aria-hidden="true">
        <span><i className="dot" style={{ background: ROLE_COLORS.performer }} />演唱</span>
        <span><i className="ring" />乐队</span>
        <span><i className="line" />一起署名</span>
        <span><i className="line dashed" />成员</span>
      </div>

      {person !== undefined && (
        <div className="graph-card glass" key={person.id}>
          <Avatar path={person.imagePath} name={person.name} size={44} />
          <div className="graph-card-text">
            <b>{person.name}</b>
            <small>
              {rolesOf(person)
                .map((r) => `${ROLE_LABELS[r] ?? r} ${person[r]}`)
                .join(" · ")}
            </small>
            <small>
              {degree} 位合作者
              {bands.length > 0 && ` · ${bands.map((b) => (b.leftAt ? `曾属 ${b.bandName}` : `${b.bandName} 成员`)).join("、")}`}
            </small>
          </div>
          <div className="graph-card-actions">
            <button className="primary" onClick={() => onOpenPerson(person.id)}>
              打开档案
            </button>
            {around === person.id ? (
              <button className="btn" onClick={() => setAround(null)}>看全部</button>
            ) : (
              <button className="btn" onClick={() => setAround(person.id)}>只看附近</button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
