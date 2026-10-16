/**
 * 一首歌的「来源与校验」：歌词哪来的、哪一版、校验发现了什么、每行时间戳靠不靠得住、
 * 分词状态和分词器、各元数据字段的来源、修订历史。
 *
 * 只有两个写操作：把旧版歌词恢复成当前版（记成新的一版，历史不抹）、手动定歌词语言。
 * 其余都是看。数据见 `jp-corpus/src/lineage.rs`。
 */

import { useCallback, useEffect, useState } from "react";

import {
  completenessLabel,
  FIELD_SOURCE_LABELS,
  issueLabel,
  LANGUAGE_LABELS,
  percent,
  qualityApi,
  REVISION_LABELS,
  SOURCE_LABELS,
  TOKEN_STATUS_LABELS,
  tokenizerLabel,
  type Issue,
  type SongLineage,
} from "../corpusQuality";
import { FIELD_LABELS } from "../libraryAdmin";

interface Props {
  songId: string;
  /** 恢复了旧版歌词：歌词行变了，调用方重拉 */
  onLyricsChanged: () => void;
  onClose: () => void;
  onError: (message: string) => void;
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));
const when = (iso: string) => (iso ? iso.replace("T", " ").replace(/(:\d\d)?Z$|\+00:00$/, "") : "—");
const LOW_TIME_CONFIDENCE = 0.5;

const FIELD_NAMES: Record<string, string> = {
  ...FIELD_LABELS,
  language: "语言",
  duration_sec: "时长",
  release_date: "发行日期",
};

export function SongLineagePanel({ songId, onLyricsChanged, onClose, onError }: Props) {
  const [data, setData] = useState<SongLineage | null>(null);
  const [raw, setRaw] = useState<{ versionId: number; text: string | null } | null>(null);
  const [busy, setBusy] = useState(false);
  const [showInfo, setShowInfo] = useState(false);

  const load = useCallback(() => {
    qualityApi.songLineage(songId).then(setData).catch((e) => onError(message(e)));
  }, [songId, onError]);
  useEffect(load, [load]);

  if (!data) return <section className="lineage-panel"><p className="muted small">读取中…</p></section>;

  const current = data.versions.find((v) => v.isCurrent) ?? null;
  const issues = current?.validation?.issues ?? [];
  const serious = issues.filter((i) => i.severity !== "info");
  const minor = issues.filter((i) => i.severity === "info");
  const lowLines = data.lines.filter((l) => l.timeConfidence !== null && l.timeConfidence < LOW_TIME_CONFIDENCE).length;

  const act = async (job: () => Promise<unknown>, lyricsChanged = false) => {
    setBusy(true);
    try {
      await job();
      if (lyricsChanged) onLyricsChanged();
      load();
    } catch (err) {
      onError(message(err));
    } finally {
      setBusy(false);
    }
  };

  const viewRaw = async (versionId: number) => {
    if (raw?.versionId === versionId) return setRaw(null);
    try {
      setRaw({ versionId, text: await qualityApi.versionRaw(versionId) });
    } catch (err) {
      onError(message(err));
    }
  };

  return (
    <section className="lineage-panel">
      <div className="card-head">
        <h3>来源与校验</h3>
        <div className="card-tools">
          <button className="ghost" disabled={busy || !current} onClick={() => void act(() => qualityApi.revalidate(songId))}>
            重新校验
          </button>
          <button className="ghost" onClick={onClose}>
            收起
          </button>
        </div>
      </div>

      {!current && <p className="muted small">这首歌还没有歌词版本记录（没有歌词，或者还没补登记）。</p>}

      {current && (
        <>
          <div className="lineage-summary">
            <div>
              <b className={`completeness c-${bucket(current.completeness)}`}>{percent(current.completeness)}</b>
              <span>{completenessLabel(current.completeness)}</span>
            </div>
            <dl>
              <dt>来源</dt>
              <dd>
                {SOURCE_LABELS[current.sourceType] ?? current.sourceType}
                {current.sourceName && ` · ${current.sourceName}`}
                {current.sourceRef && <small className="muted" title={current.sourceRef}> {current.sourceRef}</small>}
              </dd>
              <dt>版本</dt>
              <dd>
                第 {current.versionNo} 版 · {when(current.importedAt)}
                {!current.hasRawText && <small className="muted">（原文当时没保存）</small>}
              </dd>
              <dt>行数</dt>
              <dd>
                {current.lineCount} 行，{current.timedLineCount} 行有时间
                {lowLines > 0 && <span className="warn">，{lowLines} 行时间戳不可靠</span>}
                {current.validation && current.validation.repeatedLineRatio > 0 && (
                  <small className="muted">（{percent(current.validation.repeatedLineRatio)} 是重复出现的行）</small>
                )}
              </dd>
              <dt>语言</dt>
              <dd>
                <select
                  className="rate"
                  value={data.language ?? ""}
                  disabled={busy}
                  onChange={(e) => void act(() => qualityApi.setLanguage(songId, e.target.value))}
                  title="按歌词的字符推断的；手动选了之后推断不再覆盖"
                >
                  {["", "ja", "en", "ko", "zh", "mixed"].map((code) => (
                    <option key={code} value={code}>
                      {LANGUAGE_LABELS[code]}
                    </option>
                  ))}
                </select>
                {data.provenance.find((p) => p.field === "language")?.source === "manual" ? (
                  <small className="muted"> 你定的</small>
                ) : (
                  current.validation && (
                    <small className="muted"> 推断（{percent(current.validation.language.confidence)}）</small>
                  )
                )}
              </dd>
              <dt>分词</dt>
              <dd>
                {Object.entries(data.tokenStatus)
                  .map(([status, n]) => `${TOKEN_STATUS_LABELS[status] ?? status} ${n}`)
                  .join(" · ") || "—"}
                {data.tokenizers.map((t) => (
                  <small key={t.id} className="muted tokenizer-line" title={`指纹 ${t.fingerprint} · 配置 ${t.configHash}`}>
                    {tokenizerLabel(t)}（{t.lineCount} 行）
                  </small>
                ))}
              </dd>
            </dl>
          </div>

          {serious.length === 0 && <p className="small">校验没有发现需要注意的问题。</p>}
          {serious.length > 0 && <IssueList issues={serious} />}
          {minor.length > 0 && (
            <p className="small">
              <button className="link-btn" onClick={() => setShowInfo((v) => !v)}>
                {showInfo ? "收起" : `另有 ${minor.length} 条提示`}
              </button>
            </p>
          )}
          {showInfo && <IssueList issues={minor} />}
        </>
      )}

      {data.versions.length > 0 && (
        <>
          <h4>歌词版本</h4>
          <table className="data compact">
            <thead>
              <tr>
                <th className="num">版</th>
                <th>来源</th>
                <th>时间</th>
                <th className="num">行</th>
                <th className="num">完整度</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {data.versions.map((v) => (
                <tr key={v.id} className={v.isCurrent ? "current" : ""}>
                  <td className="num">{v.versionNo}</td>
                  <td>
                    {SOURCE_LABELS[v.sourceType] ?? v.sourceType}
                    {v.sourceName && <small className="muted"> {v.sourceName}</small>}
                    {v.isCurrent && <span className="tag">当前</span>}
                  </td>
                  <td className="muted">{when(v.importedAt)}</td>
                  <td className="num">{v.lineCount}</td>
                  <td className="num">{percent(v.completeness)}</td>
                  <td className="row-actions">
                    {v.hasRawText && (
                      <button className="link-btn" onClick={() => void viewRaw(v.id)}>
                        {raw?.versionId === v.id ? "收起原文" : "原文"}
                      </button>
                    )}
                    {!v.isCurrent && v.hasRawText && (
                      <button
                        className="link-btn"
                        disabled={busy}
                        onClick={() => void act(() => qualityApi.restoreVersion(songId, v.id), true)}
                        title="用这一版的原文重新挂一次歌词（记成新的一版）；校正过的分词会按原文套回去"
                      >
                        恢复这一版
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {raw && <pre className="raw-lyrics">{raw.text ?? "（这一版没有保存原文）"}</pre>}
        </>
      )}

      {data.provenance.length > 0 && (
        <>
          <h4>字段来源</h4>
          <ul className="provenance-list small">
            {data.provenance.map((p) => (
              <li key={p.field}>
                <span>{FIELD_NAMES[p.field] ?? p.field}</span>
                <span>
                  {FIELD_SOURCE_LABELS[p.source] ?? p.source}
                  {p.confidence !== null && <small className="muted"> 置信度 {percent(p.confidence)}</small>}
                </span>
                <small className="muted">{when(p.updatedAt)}</small>
              </li>
            ))}
          </ul>
        </>
      )}

      {data.revisions.length > 0 && (
        <>
          <h4>修订历史</h4>
          <ul className="revision-list small">
            {data.revisions.map((r) => (
              <li key={r.id}>
                <span className="muted">{when(r.createdAt)}</span>
                <span>
                  {REVISION_LABELS[r.kind] ?? r.kind}
                  {r.lineIdx !== null && ` · 第 ${r.lineIdx + 1} 行`}
                </span>
                <small className="muted">{revisionDetail(r.kind, r.after)}</small>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

function IssueList({ issues }: { issues: Issue[] }) {
  return (
    <ul className="issue-list small">
      {issues.map((issue, i) => (
        <li key={`${issue.code}-${issue.lineIdx ?? "all"}-${i}`} className={`sev-${issue.severity}`}>
          <span className="issue-code">{issueLabel(issue.code)}</span>
          {issue.lineIdx !== undefined && <span className="muted">第 {issue.lineIdx + 1} 行</span>}
          <span className="muted">{issue.message}</span>
        </li>
      ))}
    </ul>
  );
}

function bucket(value: number | null): string {
  if (value === null) return "none";
  if (value >= 0.95) return "good";
  if (value >= 0.8) return "ok";
  return "bad";
}

/** 修订记录的一句补充说明 */
function revisionDetail(kind: string, after: unknown): string {
  if (!after || typeof after !== "object") return "";
  const a = after as Record<string, unknown>;
  switch (kind) {
    case "lyrics_attached":
    case "lyrics_replaced":
      return `${SOURCE_LABELS[String(a.sourceType)] ?? a.sourceType ?? ""} · ${a.lines ?? "?"} 行`;
    case "retokenized":
      return String(a.tokenizer ?? "");
    case "corrections_restored":
      return `${a.lines ?? "?"} 行`;
    case "lyrics_edited":
      return Array.isArray(after) ? `改完 ${after.length} 行` : "";
    case "metadata_edited":
      return Object.entries(a)
        .map(([k, v]) => `${FIELD_NAMES[k] ?? k}：${String(v)}`)
        .join("，");
    default:
      return "";
  }
}
