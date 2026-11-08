/**
 * 改读音（#63）：歌词里右键一个词，就地改它的振假名。
 *
 * 当て字、義訓（「他人」唱ひと、「本気」唱マジ）自动注音猜不出来，这里改成对的。
 * 默认只管这首歌里同一写法，勾上「全库」所有歌里这个写法都这么读。
 * 「恢复自动」去掉改过的读音。只改显示的读音，不改分词、原形和统计——要改分词还是「校正分词」。
 */

import { useEffect, useRef, useState } from "react";

import { command } from "../api";
import type { ReadingRule } from "../generated/jp-api";
import { tr } from "../i18n";

export interface ReadingTarget {
  songId: string;
  /** 词的写法 */
  surface: string;
  /** 现在显示的读音（没有注音时是空的） */
  reading: string;
  /** 右键的位置：框贴着它弹出来 */
  x: number;
  y: number;
}

interface Props {
  target: ReadingTarget;
  /** 改了（或恢复了）之后：重新取这首歌的振假名 */
  onChanged: () => void;
  onClose: () => void;
  onError: (message: string) => void;
}

export function ReadingEditor({ target, onChanged, onClose, onError }: Props) {
  const [reading, setReading] = useState(target.reading);
  const [wholeLibrary, setWholeLibrary] = useState(false);
  const [rule, setRule] = useState<ReadingRule | null>(null);
  const [busy, setBusy] = useState(false);
  const box = useRef<HTMLFormElement>(null);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let alive = true;
    command("reading_rule", { songId: target.songId, surface: target.surface }).then(
      (found) => {
        if (!alive || found === null) return;
        setRule(found);
        setReading(found.reading);
        setWholeLibrary(found.wholeLibrary);
      },
      (e: unknown) => onError(String(e)),
    );
    input.current?.select();
    return () => {
      alive = false;
    };
  }, [target.songId, target.surface, onError]);

  // 点框外面、按 Esc 关掉
  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  const run = async (job: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await job();
      onChanged();
      onClose();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    run(() =>
      command("reading_rule_set", {
        songId: target.songId,
        surface: target.surface,
        reading: reading.trim(),
        wholeLibrary,
      }),
    );
  const restore = () => run(() => command("reading_rule_clear", { songId: target.songId, surface: target.surface }));

  // 别贴出窗口
  const left = Math.min(target.x, window.innerWidth - 300);
  const top = Math.min(target.y + 8, window.innerHeight - 170);

  return (
    <form
      ref={box}
      className="reading-editor"
      style={{ left, top }}
      onSubmit={(e) => {
        e.preventDefault();
        if (reading.trim() !== "" && !busy) void save();
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      <label className="reading-editor-head">
        <span>{tr("改读音")}</span>
        <b lang="ja">{target.surface}</b>
      </label>
      <input
        ref={input}
        type="text"
        lang="ja"
        value={reading}
        onChange={(e) => setReading(e.target.value)}
        placeholder={tr("怎么读")}
        aria-label={tr("读音")}
        spellCheck={false}
        autoFocus
      />
      <label className="reading-editor-scope">
        <input type="checkbox" checked={wholeLibrary} onChange={(e) => setWholeLibrary(e.target.checked)} />
        {tr("全库里「{surface}」都这么读", { surface: target.surface })}
      </label>
      <div className="reading-editor-actions">
        <button type="button" className="link-btn" disabled={busy || rule === null} onClick={() => void restore()}
          title={rule?.wholeLibrary ? tr("去掉全库的这条读音") : tr("去掉这首歌里改过的读音")}>
          {tr("恢复自动")}
        </button>
        <button type="button" className="btn" onClick={onClose}>{tr("取消")}</button>
        <button type="submit" className="btn primary" disabled={busy || reading.trim() === ""}>{tr("保存")}</button>
      </div>
    </form>
  );
}
