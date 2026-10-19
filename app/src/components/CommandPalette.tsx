/**
 * 命令面板 / 全局搜索。
 *
 * 要求书第十四条：Cmd+K 全局搜索，Cmd+P 命令面板。
 * 两者共用一个组件——它们的交互完全一样（浮层 + 输入 + 键盘选择），
 * 差别只在候选来自哪里。开头打 `>` 可以在搜索模式里切到命令模式，
 * 和多数编辑器的约定一致。
 *
 * 搜索是**远端**的（跨全库五种实体），所以要防抖；
 * 命令是**本地**的固定列表，即时过滤。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { api, formatDuration, POS_LABELS, ROLE_LABELS, type QuickHit } from "../api";
import { tr } from "../i18n";

/** 一条可执行的命令。 */
export interface PaletteCommand {
  id: string;
  label: string;
  hint?: string;
  /** 禁用原因。有值时命令仍然列出但不可执行——
   *  藏起来会让用户以为功能不存在，标灰能说明为什么用不了。 */
  disabled?: string;
  run: () => void | Promise<void>;
}

export type PaletteMode = "search" | "command";

interface Props {
  mode: PaletteMode;
  commands: PaletteCommand[];
  onClose: () => void;
  onPickHit: (hit: QuickHit) => void | Promise<void>;
}

/** 搜索防抖。打字时每敲一下就发一次查询会让 IPC 排队。 */
const DEBOUNCE_MS = 140;

/**
 * 全库搜索：防抖 + 丢弃过期响应。命令面板和首页的搜索栏共用。
 * `query` 为空串时不发请求、结果清空。
 */
export function useQuickSearch(query: string, perKind = 6): { hits: QuickHit[]; busy: boolean } {
  const [hits, setHits] = useState<QuickHit[]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (query.length === 0) {
      setHits([]);
      setBusy(false);
      return;
    }
    let alive = true;
    setBusy(true);
    const timer = setTimeout(() => {
      api
        .quickSearch(query, perKind)
        .then((r) => {
          // 慢的那次请求可能后到，不能让它覆盖新结果
          if (!alive) return;
          setHits([...r.tracks, ...r.people, ...r.albums, ...r.words, ...r.lyrics]);
        })
        .catch(() => alive && setHits([]))
        .finally(() => alive && setBusy(false));
    }, DEBOUNCE_MS);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [query, perKind]);
  return { hits, busy };
}

export function CommandPalette({ mode: initialMode, commands, onClose, onPickHit }: Props) {
  const [text, setText] = useState(initialMode === "command" ? ">" : "");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // `>` 开头 = 命令模式，和编辑器的约定一致
  const commandMode = text.startsWith(">");
  const query = commandMode ? text.slice(1).trim() : text.trim();
  const { hits, busy } = useQuickSearch(commandMode ? "" : query);

  useEffect(() => {
    inputRef.current?.focus();
    // 命令模式下把光标放到 `>` 后面
    inputRef.current?.setSelectionRange(text.length, text.length);
    // 只在挂载时做一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 新结果到了，选中回到第一条
  useEffect(() => setActive(0), [hits]);

  const visibleCommands = useMemo(() => {
    if (!commandMode) return [];
    const q = query.toLowerCase();
    if (!q) return commands;
    return commands.filter(
      (c) => c.label.toLowerCase().includes(q) || (c.hint ?? "").toLowerCase().includes(q),
    );
  }, [commandMode, commands, query]);

  const count = commandMode ? visibleCommands.length : hits.length;

  const pick = useCallback(
    async (index: number) => {
      if (commandMode) {
        const command = visibleCommands[index];
        if (!command || command.disabled) return;
        onClose();
        await command.run();
      } else {
        const hit = hits[index];
        if (!hit) return;
        onClose();
        await onPickHit(hit);
      }
    },
    [commandMode, visibleCommands, hits, onClose, onPickHit],
  );

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setActive((i) => (count === 0 ? 0 : (i + 1) % count));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActive((i) => (count === 0 ? 0 : (i - 1 + count) % count));
      } else if (e.key === "Enter") {
        e.preventDefault();
        void pick(active);
      }
    },
    [count, active, pick, onClose],
  );

  // 键盘移动时把选中项滚进视野
  useEffect(() => {
    listRef.current
      ?.querySelector('[data-active="true"]')
      ?.scrollIntoView({ block: "nearest" });
  }, [active]);

  return (
    <div className="palette-backdrop" onMouseDown={onClose}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          className="palette-input"
          value={text}
          placeholder={
            commandMode ? tr("输入命令…") : tr("搜索曲目 / 歌手 / 专辑 / 词 / 歌词（打 > 切到命令）")
          }
          onChange={(e) => {
            setText(e.target.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
        />

        <div className="palette-list" ref={listRef}>
          {commandMode &&
            visibleCommands.map((c, i) => (
              <button
                key={c.id}
                className={`palette-row ${c.disabled ? "off" : ""}`}
                data-active={i === active}
                onMouseEnter={() => setActive(i)}
                onClick={() => void pick(i)}
                disabled={!!c.disabled}
              >
                <span className="palette-label">{c.label}</span>
                <span className="palette-hint">{c.disabled ?? c.hint ?? ""}</span>
              </button>
            ))}

          {!commandMode &&
            hits.map((hit, i) => (
              <button
                key={hitKey(hit)}
                className="palette-row"
                data-active={i === active}
                onMouseEnter={() => setActive(i)}
                onClick={() => void pick(i)}
              >
                <span className="palette-kind">{KIND_LABELS[hit.kind]}</span>
                <span className="palette-label">{hitLabel(hit)}</span>
                <span className="palette-hint">{hitHint(hit)}</span>
              </button>
            ))}

          {!commandMode && query.length > 0 && !busy && hits.length === 0 && (
            <p className="muted pad">{tr("没有结果")}</p>
          )}
          {!commandMode && query.length === 0 && (
            <p className="muted pad">{tr("输入关键词搜索全库；打")}{" "}<code>&gt;</code>{" "}{tr("切换到命令模式。")}</p>
          )}
          {commandMode && visibleCommands.length === 0 && (
            <p className="muted pad">{tr("没有匹配的命令")}</p>
          )}
        </div>

        <div className="palette-footer muted">
          <span>{tr("↑↓ 选择")}</span>
          <span>{tr("Enter 执行")}</span>
          <span>{tr("Esc 关闭")}</span>
        </div>
      </div>
    </div>
  );
}

export const KIND_LABELS: Record<QuickHit["kind"], string> = {
  track: tr("曲目"),
  album: tr("专辑"),
  person: tr("人物"),
  word: tr("词"),
  lyric: tr("歌词"),
};

export function hitKey(hit: QuickHit): string {
  switch (hit.kind) {
    case "track":
      return `t-${hit.songId}`;
    case "album":
      return `a-${hit.albumId}`;
    case "person":
      return `p-${hit.personId}`;
    case "word":
      return `w-${hit.lemma}-${hit.pos}`;
    case "lyric":
      return `l-${hit.utteranceId}`;
  }
}

export function hitLabel(hit: QuickHit): string {
  switch (hit.kind) {
    case "track":
      return hit.title;
    case "album":
      return hit.title;
    case "person":
      return hit.name;
    case "word":
      return hit.lemma;
    case "lyric":
      return hit.text;
  }
}

export function hitHint(hit: QuickHit): string {
  switch (hit.kind) {
    case "track":
      return [hit.artist, hit.album, formatDuration(hit.durationSec)]
        .filter((x) => x && x !== "—")
        .join(" · ");
    case "album":
      return tr("{albumArtist} · {trackCount} 首", { albumArtist: hit.albumArtist, trackCount: hit.trackCount });
    case "person":
      return tr("{map} · {trackCount} 首", { map: hit.roles.map((r) => ROLE_LABELS[r] ?? r).join(" / "), trackCount: hit.trackCount });
    case "word":
      return tr("{v} · {freq} 次 · {songCount} 首", { v: POS_LABELS[hit.pos] ?? hit.pos, freq: hit.freq, songCount: hit.songCount });
    case "lyric":
      return `${hit.artist} · ${hit.title}`;
  }
}
