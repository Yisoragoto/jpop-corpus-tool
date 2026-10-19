/**
 * 语料质量总览：歌词从哪来、完整度、校验问题的分布、分词状态、用过哪些分词器，
 * 以及最需要看一眼的那几首歌（点了打开，在「⋯ → 来源与校验」里看细节）。
 */

import { useEffect, useState } from "react";

import {
  issueLabel,
  LANGUAGE_LABELS,
  percent,
  qualityApi,
  SOURCE_LABELS,
  TOKEN_STATUS_LABELS,
  tokenizerLabel,
  type QualityOverview,
} from "../corpusQuality";
import { tr } from "../i18n";

interface Props {
  onOpenSong: (songId: string) => void;
  onError: (message: string) => void;
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

export function QualityCard({ onOpenSong, onError }: Props) {
  const [data, setData] = useState<QualityOverview | null>(null);
  const [busy, setBusy] = useState(false);
  const [showAll, setShowAll] = useState(false);

  const load = () => qualityApi.overview(100).then(setData).catch((e) => onError(message(e)));
  useEffect(() => {
    qualityApi.overview(100).then(setData).catch((e) => onError(message(e)));
  }, [onError]);

  if (!data) return null;
  const worst = showAll ? data.worst : data.worst.slice(0, 8);

  return (
    <section className="card quality">
      <div className="card-head">
        <h2>{tr("语料质量")}</h2>
        <div className="card-tools">
          <button
            className="ghost"
            disabled={busy}
            title={tr("拿库里现有的歌词重新跑一遍校验（补了歌曲时长之后，「超出歌长」「提前结束」这类检查才算得出来）")}
            onClick={async () => {
              setBusy(true);
              try {
                await qualityApi.revalidate();
                await load();
              } catch (err) {
                onError(message(err));
              } finally {
                setBusy(false);
              }
            }}
          >
            {busy ? tr("校验中…") : tr("全部重新校验")}
          </button>
        </div>
      </div>
      <p className="muted small">{tr("每份歌词挂上时都会记下来源和原文，并检查漏行、重复、时间轴漂移、乱码。完整度是估计值， 统计时可以用上面的「完整度」条件把可疑的歌排除在外。")}</p>

      <div className="report-cards">
        <div className="report-card">
          <b>{percent(data.avgCompleteness)}</b>
          <span>{tr("平均完整度")}</span>
          <small>{tr("{songsWithVersion} / {songsWithLyrics} 首有版本记录", { songsWithVersion: data.songsWithVersion, songsWithLyrics: data.songsWithLyrics })}</small>
        </div>
        <div className="report-card">
          <b>{data.worst.length}</b>
          <span>{tr("需要看一眼")}</span>
          <small>{tr("有错误，或完整度低于 80%")}</small>
        </div>
        <div className="report-card">
          <b>{data.linesLowTimeConfidence}</b>
          <span>{tr("时间戳不可靠的行")}</span>
          <small>{tr("置信度低于 50%")}</small>
        </div>
        <div className="report-card">
          <b>{data.legacyVersions}</b>
          <span>{tr("老库补登记")}</span>
          <small>{tr("原文和分词器当时没有记录")}</small>
        </div>
      </div>

      <div className="quality-facts small">
        <p>
          <span className="muted">{tr("歌词来源")}</span>
          {data.bySource.map(([s, n]) => (
            <span key={s} className="chip">
              {SOURCE_LABELS[s] ?? s} {n}
            </span>
          ))}
        </p>
        <p>
          <span className="muted">{tr("语言")}</span>
          {data.byLanguage.map(([l, n]) => (
            <span key={l} className="chip">
              {LANGUAGE_LABELS[l] ?? l} {n}
            </span>
          ))}
        </p>
        <p>
          <span className="muted">{tr("分词状态")}</span>
          {data.tokenStatus.map(([s, n]) => (
            <span key={s} className="chip">
              {TOKEN_STATUS_LABELS[s] ?? s} {n}
            </span>
          ))}
        </p>
        {data.issues.length > 0 && (
          <p>
            <span className="muted">{tr("问题")}</span>
            {data.issues.map(([code, n]) => (
              <span key={code} className="chip" title={tr("{n} 首歌有这个问题", { n })}>
                {issueLabel(code)} {n}
              </span>
            ))}
          </p>
        )}
        <p>
          <span className="muted">{tr("分词器")}</span>
          {data.tokenizers.length === 0 && <span className="muted">{tr("还没有记录（老库导入的分词不知道用的哪个）")}</span>}
          {data.tokenizers.map((t) => (
            <span key={t.id} className="chip" title={tr("指纹 {fingerprint}\n配置 {configHash}\n首次使用 {firstUsedAt}", { fingerprint: t.fingerprint, configHash: t.configHash, firstUsedAt: t.firstUsedAt })}>{tr("{t} · {lineCount} 行", { t: tokenizerLabel(t), lineCount: t.lineCount })}</span>
          ))}
        </p>
      </div>

      {worst.length > 0 && (
        <table className="data compact">
          <thead>
            <tr>
              <th>{tr("歌")}</th>
              <th>{tr("来源")}</th>
              <th className="num">{tr("完整度")}</th>
              <th>{tr("问题")}</th>
            </tr>
          </thead>
          <tbody>
            {worst.map((s) => (
              <tr key={s.songId}>
                <td>
                  <button className="word-link" onClick={() => onOpenSong(s.songId)}>
                    {s.title}
                  </button>
                  <small className="muted"> {s.artist}</small>
                </td>
                <td className="muted">{SOURCE_LABELS[s.sourceType] ?? s.sourceType}</td>
                <td className="num">{percent(s.completeness)}</td>
                <td className="small">{s.codes.map(issueLabel).join(tr("、"))}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {data.worst.length > 8 && (
        <button className="link-btn" onClick={() => setShowAll((v) => !v)}>
          {showAll ? tr("收起") : tr("全部 {length} 首", { length: data.worst.length })}
        </button>
      )}
    </section>
  );
}
