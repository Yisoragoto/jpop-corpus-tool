/**
 * 逐行编辑歌词：改错字、补漏的行、删重复贴的行、挪时间戳。
 *
 * 存下来是**新的一版歌词**（来源「逐行编辑」），旧版原文留着、能在「来源与校验」里恢复；
 * 修订历史里记下改动前后的全部行。没改的行，校正过的分词会按原文套回去。
 * 后端见 `jp-app/src/lyrics.rs::save_edited_lines`。
 */

import { useMemo, useState } from "react";

import type { LyricLine } from "../api";
import { qualityApi } from "../corpusQuality";
import { formatStamp, parseStamp, summarizeEdit, type EditRow } from "../lyricEdit";

interface Props {
  songId: string;
  lines: LyricLine[];
  onSaved: (message: string) => void;
  onCancel: () => void;
  onError: (message: string) => void;
}

interface Draft {
  key: number;
  stamp: string;
  text: string;
}

let nextKey = 1;
const draftOf = (timeSec: number | null, text: string): Draft => ({ key: nextKey++, stamp: formatStamp(timeSec), text });

export function LyricsEditor({ songId, lines, onSaved, onCancel, onError }: Props) {
  const original: EditRow[] = useMemo(() => lines.map((l) => ({ timeSec: l.timeSec, text: l.text })), [lines]);
  const [rows, setRows] = useState<Draft[]>(() => lines.map((l) => draftOf(l.timeSec, l.text)));
  const [note, setNote] = useState("");
  const [saving, setSaving] = useState(false);

  const parsed = rows.map((r) => parseStamp(r.stamp));
  const badRows = parsed.flatMap((p, i) => (p === undefined ? [i] : []));
  const after: EditRow[] = rows.map((r, i) => ({ timeSec: parsed[i] ?? null, text: r.text }));
  const summary = summarizeEdit(original, after);
  const unchanged = summary === "没有改动";

  const update = (i: number, patch: Partial<Draft>) => setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const insertAfter = (i: number) =>
    setRows((rs) => {
      const at = parsed[i] ?? null;
      return [...rs.slice(0, i + 1), draftOf(at, ""), ...rs.slice(i + 1)];
    });
  const remove = (i: number) => setRows((rs) => rs.filter((_, j) => j !== i));

  const save = async () => {
    setSaving(true);
    try {
      const done = await qualityApi.saveLines(
        songId,
        after.filter((r) => r.text.trim() !== ""),
        note,
      );
      onSaved(`歌词存成了新的一版：${done.lyricLines} 行${done.correctionsRestored ? `，套回 ${done.correctionsRestored} 行分词校正` : ""}`);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="lyrics-editor">
      <div className="card-head">
        <h3>逐行编辑歌词</h3>
        <span className="muted small">{summary}</span>
      </div>
      <p className="muted small">
        时间写成 <code>1:02.50</code>，留空表示这一行没有时间；文字留空的行会被去掉。
        存下来是新的一版，旧版能在「来源与校验」里恢复。
      </p>
      <div className="lyrics-editor-rows">
        {rows.map((r, i) => (
          <div key={r.key} className={`lyrics-editor-row${parsed[i] === undefined ? " bad" : ""}`}>
            <span className="muted small num">{i + 1}</span>
            <input
              type="text"
              className="stamp"
              value={r.stamp}
              placeholder="无"
              aria-label={`第 ${i + 1} 行的时间`}
              onChange={(e) => update(i, { stamp: e.target.value })}
            />
            <input
              type="text"
              className="line-text"
              value={r.text}
              aria-label={`第 ${i + 1} 行`}
              onChange={(e) => update(i, { text: e.target.value })}
            />
            <button className="link-btn" title="在下面插一行" onClick={() => insertAfter(i)}>
              ＋
            </button>
            <button className="link-btn danger" title="删掉这一行" onClick={() => remove(i)}>
              ✕
            </button>
          </div>
        ))}
        {rows.length === 0 && (
          <button className="link-btn" onClick={() => setRows([draftOf(null, "")])}>
            加一行
          </button>
        )}
      </div>
      {badRows.length > 0 && <p className="warn small">第 {badRows.map((i) => i + 1).join("、")} 行的时间认不出来</p>}
      <div className="lyrics-editor-actions">
        <input type="text" placeholder="改了什么（可不填，记进修订历史）" value={note} onChange={(e) => setNote(e.target.value)} />
        <button className="ghost" onClick={onCancel} disabled={saving}>
          取消
        </button>
        <button className="primary" onClick={() => void save()} disabled={saving || unchanged || badRows.length > 0}>
          {saving ? "保存中…" : "存成新的一版"}
        </button>
      </div>
    </section>
  );
}
