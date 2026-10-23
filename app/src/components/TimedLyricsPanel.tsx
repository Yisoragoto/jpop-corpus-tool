import { useCallback, useEffect, useRef, useState } from "react";

import {
  api, LYRICS_SOURCE_LABELS,
  type AttachedLyrics, type TimedCandidate, type TimedSearch,
  type TimedSourceConfig, type TimedSourceSettings, type TimedTarget,
} from "../api";
import { tr, trBackend } from "../i18n";

interface Props {
  songId?: string;
  onChanged: () => void;
  onError: (message: string) => void;
  onApplied?: (result: AttachedLyrics) => void;
}

const SOURCE_KEYS = ["netease", "qq", "kugou", "amll", "lrclib"] as const;

/** 有字级计数才标逐字；扩展精度字段缺省时，不把普通歌词格式当作真实逐字。 */
export function candidatePrecision(candidate: Pick<TimedCandidate, "timedLineCount" | "format" | "mode">): "word" | "line" | "plain" | "lineOrPlain" {
  if (candidate.mode === "plain") return "plain";
  if (candidate.mode === "lineSynced") return "line";
  if (Number.isFinite(candidate.timedLineCount) && candidate.timedLineCount > 0) return "word";
  const format = candidate.format.toLowerCase();
  if (["plain", "txt", "text", "plaintext", "plain_text"].includes(format)) return "plain";
  return format === "lrc" ? "line" : "lineOrPlain";
}

const PRECISION_LABELS = {
  word: tr("含真实逐字时间"), line: tr("逐行"), plain: tr("纯文本"), lineOrPlain: tr("逐行 / 纯文本"),
};
function sourceLabel(source: string): string {
  if (source === "netease") return tr("网易云音乐");
  return LYRICS_SOURCE_LABELS[source as keyof typeof LYRICS_SOURCE_LABELS] ?? source;
}
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

/** 换歌时销毁旧请求的展示状态；回调身份变化不会触发任何搜索。 */
export function TimedLyricsPanel(props: Props) {
  return <TimedLyricsWorkspace key={props.songId ?? "roster"} {...props} />;
}

function TimedLyricsWorkspace(props: Props) {
  const callbacks = useRef(props);
  callbacks.current = props;
  const [targets, setTargets] = useState<TimedTarget[]>([]);
  const [selected, setSelected] = useState("");
  const [loading, setLoading] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [applying, setApplying] = useState(false);
  const [savingSources, setSavingSources] = useState(false);
  const [revision, setRevision] = useState(0);
  const [note, setNote] = useState("");
  const alive = useRef(false);
  const rosterRequest = useRef(0);
  const rosterBusy = useRef(false);

  const refreshTargets = useCallback(async () => {
    if (rosterBusy.current) return;
    rosterBusy.current = true;
    const request = ++rosterRequest.current;
    setLoading(true);
    try {
      const next = await api.lyricsTimedTargets();
      if (!alive.current || request !== rosterRequest.current) return;
      setTargets(next);
      setSelected((current) => next.some((song) => song.songId === current) ? current : next[0]?.songId ?? "");
      setLoaded(true);
    } catch (error) {
      if (alive.current && request === rosterRequest.current) callbacks.current.onError(errorText(error));
    } finally {
      if (alive.current && request === rosterRequest.current) {
        rosterBusy.current = false;
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    if (props.songId === undefined) void refreshTargets();
    return () => {
      alive.current = false;
      ++rosterRequest.current;
      rosterBusy.current = false;
    };
  }, [props.songId, refreshTargets]);

  const songId = props.songId ?? selected;
  const target = targets.find((song) => song.songId === songId);
  const applied = (result: AttachedLyrics) => {
    setNote(tr("已采用 {source} 的歌词，共 {lines} 行。", { source: sourceLabel(result.source), lines: result.lyricLines }));
    if (props.songId === undefined) void refreshTargets();
    callbacks.current.onApplied?.(result);
    callbacks.current.onChanged();
  };

  return (
    <section className="timed-lyrics" aria-label={tr("逐字歌词")}>
      <h3>{props.songId === undefined ? tr("逐首预览与更换") : tr("逐字歌词")}</h3>
      <p className="timed-hint">{tr("单首更换可先预览；批量任务仅自动采用身份和正文验证通过的结果。逐字行数不含估算。")}</p>
      {props.songId === undefined && (
        <>
          <p className="timed-hint">{tr("此处包含缺歌词或逐字时间不完整的歌曲；可一键批量补全，也可逐首预览更换。")}</p>
          <div className="timed-toolbar">
            <label className="timed-song-picker">
              <span>{tr("选择歌曲")}</span>
              <select value={selected} disabled={loading || applying || targets.length === 0} onChange={(event) => { setSelected(event.target.value); setNote(""); }}>
                {targets.length === 0 && <option value="">{loading ? tr("加载中…") : tr("没有待补逐字歌词的歌曲")}</option>}
                {targets.map((song) => <option key={song.songId} value={song.songId}>{song.title} — {song.artist}</option>)}
              </select>
            </label>
            <button type="button" className="btn" disabled={loading || applying} onClick={() => void refreshTargets()}>{loading ? tr("加载中…") : tr("刷新名单")}</button>
          </div>
          {loaded && !loading && targets.length === 0 && <p className="timed-hint">{tr("没有待补逐字歌词的歌曲")}</p>}
          {target && <p className="timed-hint">{tr("现有 {lines} 行，真实逐字 {timed} 行", { lines: target.lyricLines, timed: target.timedLines })}</p>}
        </>
      )}
      <TimedSources
        disabled={applying}
        onError={(message) => callbacks.current.onError(message)}
        onBusy={setSavingSources}
        onSaved={() => { setRevision((value) => value + 1); setNote(""); }}
      />
      {songId !== "" && (
        <TimedSongSearch
          key={`${songId}:${revision}`} songId={songId}
          disabled={loading || savingSources} onApplying={setApplying}
          onApplied={applied} onError={(message) => callbacks.current.onError(message)}
        />
      )}
      {note && <p className="timed-hint" role="status">{note}</p>}
    </section>
  );
}

function TimedSongSearch({ songId, disabled, onApplying, onApplied, onError }: {
  songId: string;
  disabled: boolean;
  onApplying: (busy: boolean) => void;
  onApplied: (result: AttachedLyrics) => void;
  onError: (message: string) => void;
}) {
  const [result, setResult] = useState<TimedSearch | null>(null);
  const [busy, setBusy] = useState("");
  const pending = useRef(false);
  const alive = useRef(false);
  const callbacks = useRef({ onApplying, onApplied, onError });
  callbacks.current = { onApplying, onApplied, onError };
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; };
  }, []);

  const search = async () => {
    if (pending.current || disabled) return;
    pending.current = true;
    setBusy("search");
    setResult(null);
    try {
      const next = await api.lyricsTimedSearch(songId);
      if (alive.current && next.songId === songId) setResult(next);
    } catch (error) {
      if (alive.current) callbacks.current.onError(errorText(error));
    } finally {
      if (alive.current) { pending.current = false; setBusy(""); }
    }
  };

  const apply = async (candidate: TimedCandidate) => {
    if (pending.current || disabled || result?.songId !== songId) return;
    pending.current = true;
    setBusy(candidate.id);
    callbacks.current.onApplying(true);
    try {
      const attached = await api.lyricsTimedApply(songId, candidate.id);
      if (!alive.current || attached.songId !== songId) return;
      setResult(null);
      callbacks.current.onApplied(attached);
    } catch (error) {
      if (alive.current) {
        // 过期候选也在后端拒绝；失败后重新搜索，不能再次采用这批候选。
        setResult(null);
        callbacks.current.onError(errorText(error));
      }
    } finally {
      if (alive.current) {
        pending.current = false;
        setBusy("");
        callbacks.current.onApplying(false);
      }
    }
  };

  return (
    <div className="timed-search" aria-busy={busy !== ""}>
      <button type="button" className="btn" disabled={disabled || busy !== ""} onClick={() => void search()}>{busy === "search" ? tr("正在搜…") : tr("搜索逐字歌词")}</button>
      {result && (
        <>
          <p className="timed-hint">{result.title} · {tr("现有 {lines} 行，真实逐字 {timed} 行", { lines: result.currentLines, timed: result.currentTimedLines })}</p>
          {result.warnings.length > 0 && <ul className="timed-warnings">{result.warnings.map((warning, index) => <li key={`${warning.source}:${index}`}>{sourceLabel(warning.source)}{tr("：")}{trBackend(warning.message)}</li>)}</ul>}
          {result.candidates.length === 0 && <p className="timed-hint" role="status">{result.warnings.length > 0 ? tr("本次没有可用候选，请查看上方各来源说明后重试。") : tr("所选来源未返回候选，可调整来源后重新搜索。")}</p>}
          <div className="timed-candidates">
            {result.candidates.map((candidate) => (
              <article className="timed-candidate" key={candidate.id}>
                <header>
                  <strong>{candidate.title}</strong><span>{candidate.artist}</span>
                  <button type="button" className="btn" disabled={disabled || busy !== ""} onClick={() => void apply(candidate)}>{busy === candidate.id ? tr("采用中…") : tr("采用")}</button>
                </header>
                <p className="timed-hint">{sourceLabel(candidate.source)} · {candidate.format} · {PRECISION_LABELS[candidatePrecision(candidate)]} · {tr("{lines} 行 / 真实逐字 {timed} 行", { lines: candidate.lineCount, timed: candidate.timedLineCount })}</p>
                {candidatePrecision(candidate) !== "word" && <p className="timed-hint">{tr("此候选没有真实逐字时间；采用后按逐行或纯文本显示。")}</p>}
                {candidate.matchNote && <p className="timed-hint">{trBackend(candidate.matchNote)}</p>}
                {candidate.preview.length > 0 ? <pre className="timed-preview">{candidate.preview.slice(0, 5).join("\n")}</pre> : <p className="timed-hint">{tr("此候选没有预览文本")}</p>}
              </article>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function TimedSources({ disabled, onBusy, onSaved, onError }: {
  disabled: boolean;
  onBusy: (busy: boolean) => void;
  onSaved: () => void;
  onError: (message: string) => void;
}) {
  const [settings, setSettings] = useState<TimedSourceSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const [clearDeveloper, setClearDeveloper] = useState(false);
  const developerInput = useRef<HTMLInputElement>(null);
  const alive = useRef(false);
  const pending = useRef(false);
  const requestId = useRef(0);
  const callbacks = useRef({ onBusy, onSaved, onError });
  callbacks.current = { onBusy, onSaved, onError };

  const load = useCallback(async () => {
    if (pending.current) return;
    pending.current = true;
    const request = ++requestId.current;
    setBusy(true);
    try {
      const next = await api.lyricsSourcesGet();
      if (alive.current && request === requestId.current) setSettings({ ...next, lrclib: next.lrclib ?? true });
    } catch {
      if (alive.current && request === requestId.current) callbacks.current.onError(tr("读取歌词来源失败，请重试。"));
    } finally {
      if (alive.current && request === requestId.current) { pending.current = false; setBusy(false); }
    }
  }, []);
  useEffect(() => {
    alive.current = true;
    void load();
    return () => { alive.current = false; ++requestId.current; pending.current = false; };
  }, [load]);

  const save = async () => {
    if (!settings || disabled || pending.current) return;
    const config: TimedSourceConfig = {
      netease: settings.netease, qq: settings.qq, kugou: settings.kugou,
      amll: settings.amll, lrclib: settings.lrclib, apple: settings.apple, appleStorefront: settings.appleStorefront.trim(),
    };
    // 未输入则省略（保留后端旧值）；明确清除才发送空字符串。
    if (clearDeveloper) config.appleDeveloperToken = "";
    else if (developerInput.current?.value) config.appleDeveloperToken = developerInput.current.value;
    if (developerInput.current) developerInput.current.value = "";
    pending.current = true;
    setBusy(true);
    setNote("");
    callbacks.current.onBusy(true);
    try {
      const next = await api.lyricsSourcesSet(config);
      if (!alive.current) return;
      setSettings(next);
      setClearDeveloper(false);
      setNote(tr("来源设置已保存，请重新搜索。"));
      callbacks.current.onSaved();
    } catch {
      // 后端异常可能包含请求内容；含凭据的调用不向 UI 转发原始错误。
      if (alive.current) callbacks.current.onError(tr("保存歌词来源失败；令牌输入已清空，请重试。"));
    } finally {
      delete config.appleDeveloperToken;
      if (alive.current) {
        pending.current = false;
        setBusy(false);
        callbacks.current.onBusy(false);
      }
    }
  };

  return (
    <details className="timed-sources">
      <summary>{tr("歌词来源设置")}</summary>
      {settings === null ? <button type="button" className="btn" disabled={busy || disabled} onClick={() => void load()}>{busy ? tr("加载中…") : tr("重试读取来源")}</button> : (
        <form autoComplete="off" onSubmit={(event) => { event.preventDefault(); void save(); }}>
          <fieldset disabled={busy || disabled}>
            <div className="timed-source-checks">{SOURCE_KEYS.map((source) => <label className="check" key={source}><input type="checkbox" checked={settings[source]} onChange={(event) => setSettings({ ...settings, [source]: event.target.checked })} />{sourceLabel(source)}</label>)}</div>
            <p className="timed-hint">{tr("LRCLIB 可提供逐行或纯文本候选；只有实际解析到真实逐字时间才计入逐字行数。")}</p>
            <label className="check"><input type="checkbox" checked={settings.apple} onChange={(event) => setSettings({ ...settings, apple: event.target.checked })} />{sourceLabel("apple")}{tr("（可选，默认关闭）")}</label>
            <p className="timed-hint">{tr("Apple Music 官方 API 不提供歌词正文或逐字时间。此开关仅用于普通元数据刮削，不作为歌词来源；仅需 Developer Token。")}</p>
            <label className="timed-field"><span>{tr("Apple 商店地区")}</span><input type="text" value={settings.appleStorefront} maxLength={16} spellCheck={false} onChange={(event) => setSettings({ ...settings, appleStorefront: event.target.value })} /></label>
            <label className="timed-field"><span>Apple Developer Token · {clearDeveloper ? tr("保存时清除") : settings.appleDeveloperTokenSet ? tr("已配置") : tr("未配置")}</span><input ref={developerInput} type="password" autoComplete="new-password" spellCheck={false} onChange={() => setClearDeveloper(false)} /></label>
            <button type="button" className="btn" onClick={() => { if (developerInput.current) developerInput.current.value = ""; setClearDeveloper(true); }}>{tr("清除 Developer Token")}</button>
            <p className="timed-hint">{tr("令牌仅保存在后端内存，重启后需重新配置；留空保留已配置值，清除需保存，输入框不回显。")}</p>
            <button type="submit" className="btn">{busy ? tr("保存中…") : tr("保存来源设置")}</button>
          </fieldset>
        </form>
      )}
      {note && <p className="timed-hint" role="status">{note}</p>}
    </details>
  );
}
