/**
 * 一首歌的「歌词来源与同步」：这首歌有哪几份歌词（你导入 / 编辑的、旁边的文件、内嵌、在线）、
 * 现在用的是哪一份、译文用哪一份、整体偏移多少。
 *
 * 打开时先找一遍音频旁边的同名文件和内嵌歌词，放了新文件进去这里马上看得到。
 * 写操作只有三个：用某一份当歌词（记成你的选择，自动流程之后不换）、交还自动选择、调偏移。
 * 版式沿用「来源与校验」（SongLineagePanel）。
 */

import { useCallback, useEffect, useState } from "react";

import { tr } from "../i18n";
import {
  CHOSEN_BY_LABELS,
  formatOffset,
  KIND_LABELS,
  LOOKUP_LABELS,
  lyricSourcesApi,
  MAX_OFFSET_MS,
  PRECISION_LABELS,
  type LyricSourceRow,
  type LyricSourcesView,
  type LyricTimeline,
} from "../lyricSources";

interface Props {
  songId: string;
  /** 换了来源或偏移：歌词行的时间变了，调用方重拉 */
  onLyricsChanged: () => void;
  onClose: () => void;
  onError: (message: string) => void;
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));
const when = (iso: string) => (iso ? iso.replace("T", " ").replace(/(:\d\d)?Z$/, "") : "—");
/** 微调按钮一次挪多少（毫秒） */
const STEPS = [-500, -100, 100, 500];

export function LyricSourcesPanel({ songId, onLyricsChanged, onClose, onError }: Props) {
  const [view, setView] = useState<LyricSourcesView | null>(null);
  const [preview, setPreview] = useState<{ id: number; timeline: LyricTimeline } | null>(null);
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState("");

  const load = useCallback(() => {
    lyricSourcesApi
      .view(songId)
      .then((v) => {
        setView(v);
        setDraft(String((v.selection?.offsetMs ?? 0) / 1000));
      })
      .catch((e) => onError(message(e)));
  }, [songId, onError]);
  useEffect(load, [load]);

  if (!view) return <section className="lineage-panel"><p className="muted small">{tr("读取中…")}</p></section>;

  const selection = view.selection;
  const offset = selection?.offsetMs ?? 0;
  const originals = view.sources.filter((s) => s.role === "original");
  const translations = view.sources.filter((s) => s.role === "translation");
  const current = originals.find((s) => s.id === selection?.sourceId) ?? null;
  const recommended = view.recommended !== null && view.recommended !== selection?.sourceId
    ? originals.find((s) => s.id === view.recommended) ?? null
    : null;

  const act = async (job: () => Promise<unknown>, lyricsChanged = true) => {
    setBusy(true);
    try {
      await job();
      if (lyricsChanged) onLyricsChanged();
      setPreview(null);
      load();
    } catch (err) {
      onError(message(err));
    } finally {
      setBusy(false);
    }
  };

  const setOffset = (ms: number) => {
    const clamped = Math.max(-MAX_OFFSET_MS, Math.min(MAX_OFFSET_MS, Math.round(ms)));
    if (clamped === offset) return;
    void act(() => lyricSourcesApi.setOffset(songId, clamped));
  };

  const togglePreview = async (row: LyricSourceRow) => {
    if (preview?.id === row.id) return setPreview(null);
    try {
      setPreview({ id: row.id, timeline: await lyricSourcesApi.sourceTimeline(row.id) });
    } catch (err) {
      onError(message(err));
    }
  };

  const describe = (row: LyricSourceRow) => {
    const parts = [PRECISION_LABELS[row.precision], tr("{n} 行", { n: row.lineCount })];
    if (row.rawFormat) parts.push(row.rawFormat.toUpperCase().replace("ENHANCED_LRC", tr("逐字 LRC")));
    if (row.language) parts.push(row.language);
    return parts.join(" · ");
  };

  const unusable = (row: LyricSourceRow) =>
    !row.hasRaw ? tr("当时没有保存原文") : row.missing ? tr("文件不在了") : row.lineCount === 0 ? tr("解析不出歌词行") : null;

  return (
    <section className="lineage-panel lyric-sources">
      <div className="card-head">
        <h3>{tr("歌词来源与同步")}</h3>
        <div className="card-tools">
          {selection?.chosenBy === "user" && (
            <button className="ghost" disabled={busy} onClick={() => void act(() => lyricSourcesApi.auto(songId))}
              title={tr("不再算你选定的：按优先级（你导入或编辑的 → 旁边的文件 → 内嵌 → 在线）挑最好的一份")}>
              {tr("交还自动选择")}
            </button>
          )}
          <button className="ghost" onClick={onClose}>{tr("收起")}</button>
        </div>
      </div>

      {selection && (
        <p className="small muted">
          {current
            ? tr("现在用的是「{name}」（{kind}）。", { name: current.provider || KIND_LABELS[current.kind], kind: KIND_LABELS[current.kind] })
            : tr("现在没有选中的来源。")}
          {CHOSEN_BY_LABELS[selection.chosenBy]}
        </p>
      )}
      {recommended && (
        <p className="small">
          {tr("按优先级默认会用「{name}」（{kind}）。", { name: recommended.provider || KIND_LABELS[recommended.kind], kind: KIND_LABELS[recommended.kind] })}
          <button className="link-btn" disabled={busy} onClick={() => void act(() => lyricSourcesApi.apply(songId, recommended.id))}>{tr("用这一份")}</button>
        </p>
      )}

      <h4>{tr("原文")}</h4>
      {originals.length === 0 && <p className="muted small">{tr("没有找到任何歌词来源。可以在音频旁边放一份同名的 .lrc / .srt / .txt，或者在线补齐、自己导入。")}</p>}
      <ul className="source-list">
        {originals.map((row) => {
          const isCurrent = row.id === selection?.sourceId;
          const blocked = unusable(row);
          return (
            <li key={row.id} className={isCurrent ? "current" : ""}>
              <div>
                <b>{KIND_LABELS[row.kind]}</b>
                {row.provider && <span className="muted"> · {row.provider}</span>}
                {isCurrent && <span className="tag">{tr("当前")}</span>}
                {row.userModified && <span className="tag">{tr("你改过")}</span>}
                <small className="muted source-meta">
                  {describe(row)}{row.offsetMs !== 0 && ` · ${tr("偏移 {offset}", { offset: formatOffset(row.offsetMs) })}`}
                  {` · ${tr("更新于 {when}", { when: when(row.updatedAt) })}`}
                </small>
                {row.sourceRef && <small className="muted source-ref" title={row.sourceRef}>{row.sourceRef}</small>}
              </div>
              <div className="row-actions">
                {row.hasRaw && (
                  <button className="link-btn" onClick={() => void togglePreview(row)}>
                    {preview?.id === row.id ? tr("收起预览") : tr("预览")}
                  </button>
                )}
                {!isCurrent && (
                  <button className="link-btn" disabled={busy || blocked !== null} title={blocked ?? undefined}
                    onClick={() => void act(() => lyricSourcesApi.apply(songId, row.id))}>
                    {tr("用这一份")}
                  </button>
                )}
              </div>
              {preview?.id === row.id && <TimelinePreview timeline={preview.timeline} />}
            </li>
          );
        })}
      </ul>

      <h4>{tr("译文")}</h4>
      {translations.length === 0 ? (
        <p className="muted small">{tr("没有单独的译文文件（同名加语言后缀，例如 song.zh-Hans.lrc）。歌词文件里和原文同一时间戳的译文行会自动认出来。")}</p>
      ) : (
        <div className="segmented-row">
          <select
            value={view.translationSourceId ?? ""}
            disabled={busy}
            onChange={(e) => {
              const id = e.target.value === "" ? null : Number(e.target.value);
              void act(() => lyricSourcesApi.selectTranslation(songId, id), false);
            }}
          >
            <option value="">{tr("不用译文")}</option>
            {translations.map((row) => (
              <option key={row.id} value={row.id}>{`${row.provider}${row.language ? ` (${row.language})` : ""}`}</option>
            ))}
          </select>
        </div>
      )}

      <h4>{tr("整体偏移")}</h4>
      {!current || current.precision === "none" ? (
        <p className="muted small">{tr("这份歌词没有时间轴，偏移、跟随播放、点行跳转都用不了。")}</p>
      ) : (
        <>
          <div className="offset-row">
            {STEPS.map((step) => (
              <button key={step} className="btn" disabled={busy} onClick={() => setOffset(offset + step)}>
                {formatOffset(step)}
              </button>
            ))}
            <label className="offset-input">
              <input
                type="number"
                step="0.05"
                min={-MAX_OFFSET_MS / 1000}
                max={MAX_OFFSET_MS / 1000}
                value={draft}
                disabled={busy}
                onChange={(e) => setDraft(e.target.value)}
                onBlur={() => {
                  const sec = Number(draft);
                  if (Number.isFinite(sec)) setOffset(sec * 1000);
                  else setDraft(String(offset / 1000));
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                }}
              />
              {tr("秒")}
            </label>
            <button className="link-btn" disabled={busy || offset === 0} onClick={() => setOffset(0)}>{tr("归零")}</button>
          </div>
          <p className="muted small">
            {tr("现在 {offset}。正值让歌词晚一点出现。改的是这首歌的时间，原文不动；单句循环、制卡截音频也跟着对齐。", { offset: formatOffset(offset) })}
          </p>
        </>
      )}

      {view.lookups.length > 0 && (
        <>
          <h4>{tr("在线查询")}</h4>
          <ul className="provenance-list small">
            {view.lookups.map((l) => (
              <li key={l.provider}>
                <span>{LOOKUP_LABELS[l.status]}</span>
                <span className="muted">{l.message || (l.retryAfter ? tr("{when} 之前批量补齐不再查（单首补齐照样会查）", { when: when(l.retryAfter) }) : "")}</span>
                <small className="muted">{when(l.checkedAt)}</small>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

/** 一个来源解析出来的前几行：时间、原文、对上的译文 */
function TimelinePreview({ timeline }: { timeline: LyricTimeline }) {
  const fmt = (ms: number | null) => {
    if (ms === null) return "—";
    const s = ms / 1000;
    return `${Math.floor(s / 60)}:${(s % 60).toFixed(2).padStart(5, "0")}`;
  };
  return (
    <div className="source-preview">
      {timeline.warnings?.map((w) => <p key={w} className="warn small">{w}</p>)}
      {timeline.translation.state === "unaligned" && (
        <p className="warn small">{tr("译文和原文的时间对不上，没有合并显示（不按行号硬拼）。")}</p>
      )}
      <ol className="raw-lyrics">
        {timeline.lines.slice(0, 40).map((line) => (
          <li key={line.id}>
            <span className="muted">{fmt(line.startMs)}</span> {line.text}
            {line.translation && <span className="muted"> · {line.translation}</span>}
          </li>
        ))}
        {timeline.lines.length > 40 && <li className="muted">{tr("…共 {n} 行", { n: timeline.lines.length })}</li>}
      </ol>
    </div>
  );
}
