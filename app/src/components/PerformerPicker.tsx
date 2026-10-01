/**
 * 演唱者多选。检索页和分析页共用，对应 PyQt 版的 MultiSelectComboBox。
 *
 * 列表按曲数降序、同数按名字——和 Python `_load_artists` 的排序一致，
 * 只是来源换成了演唱署名（合作曲已按 `/` 拆开）。选中的 id 按列表顺序给出，
 * 报告里的「筛选」一行也就按这个顺序写名字。
 */

import { useEffect, useMemo, useRef, useState } from "react";

import { api, type PersonSummary } from "../api";

let cached: Promise<PersonSummary[]> | null = null;

/** 演唱者列表整个会话只取一次；取失败了下次再取 */
function loadPerformers(): Promise<PersonSummary[]> {
  if (!cached) {
    cached = api.peopleByRole("performer", 5000).catch((err) => {
      cached = null;
      throw err;
    });
  }
  return cached;
}

/** 曲库改过（编辑、删除、导入）之后调一下，下次打开重新取 */
export function invalidatePerformers() {
  cached = null;
}

/**
 * 歌手名 → 照片路径（`artists.image_path`，没有照片的是空串）。
 *
 * 和上面的下拉共用同一份缓存，所以曲库的歌手网格不会为了头像再查一遍库。
 * 名字**精确匹配**，和人物页一致；合作署名（`A/B`）在调用处退到第一位演唱者。
 */
export function useArtistPhotos(): Map<string, string> {
  const [photos, setPhotos] = useState<Map<string, string>>(new Map());
  useEffect(() => {
    let alive = true;
    loadPerformers().then(
      (people) => {
        if (alive) setPhotos(new Map(people.map((p) => [p.name, p.imagePath])));
      },
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, []);
  return photos;
}

interface Props {
  selected: number[];
  onChange: (ids: number[]) => void;
  onError?: (message: string) => void;
}

export function PerformerPicker({ selected, onChange, onError }: Props) {
  const [people, setPeople] = useState<PersonSummary[]>([]);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    loadPerformers()
      .then((list) => alive && setPeople(list))
      .catch((err) => onError?.(err instanceof Error ? err.message : String(err)));
    return () => {
      alive = false;
    };
  }, [onError]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const chosen = useMemo(() => new Set(selected), [selected]);
  const names = useMemo(
    () => people.filter((p) => chosen.has(p.id)).map((p) => p.name),
    [people, chosen],
  );
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? people.filter((p) => p.name.toLowerCase().includes(q)) : people;
  }, [people, query]);

  const toggle = (id: number) => {
    const next = new Set(chosen);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onChange(people.filter((p) => next.has(p.id)).map((p) => p.id));
  };

  const label =
    selected.length === 0
      ? "全部歌手"
      : names.length === 1
        ? names[0]
        : `${names[0] ?? "…"} 等 ${selected.length} 人`;

  return (
    <div className="picker" ref={root}>
      <button
        className={`picker-button${selected.length ? " on" : ""}`}
        onClick={() => setOpen((v) => !v)}
        title={names.length > 1 ? names.join("、") : undefined}
      >
        {label} ▾
      </button>
      {open && (
        <div className="picker-pop">
          <div className="picker-head">
            <input
              className="search"
              placeholder="筛选歌手"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              autoFocus
            />
            <button className="link-btn" disabled={!selected.length} onClick={() => onChange([])}>
              清除
            </button>
          </div>
          <div className="picker-list">
            {visible.map((p) => (
              <label key={p.id} className="picker-item">
                <input type="checkbox" checked={chosen.has(p.id)} onChange={() => toggle(p.id)} />
                <span className="picker-name">{p.name}</span>
                <span className="muted small">{p.trackCount} 首</span>
              </label>
            ))}
            {visible.length === 0 && <p className="muted small pad">没有匹配的歌手</p>}
          </div>
        </div>
      )}
    </div>
  );
}
