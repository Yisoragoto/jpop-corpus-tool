/**
 * Anki 页。架构图上那个 Anki 盒子的界面。
 *
 * 这套卡片和现成词库的区别只有一条：**例句是用户自己听的歌**。
 * 所以选词表按语料频次排、预览里直接看例句，而不是先让人填一堆选项。
 *
 * 三件事：
 *
 * 1. **Anki 没开也能用大半。** 选词、预览、看学习状态都只读本地文件，
 *    只有真正推卡片那一步需要 Anki 开着。所以连不上时不报红，
 *    只在导出按钮那里说清楚。
 * 2. **已经学过的默认不再导。** 学习状态是直接读 collection.anki2 拿的。
 * 3. **重复不是错误。** 同一个词在别的歌里又遇到了，追加例句到已有的卡上。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import {
  ANKI_OUTCOME_LABELS,
  api,
  POS_LABELS,
  type AnkiCard,
  type AnkiDupMode,
  type AnkiDupScope,
  type AnkiProgress,
  type AnkiRefreshScope,
  type AnkiStatus,
  type WordCandidate,
} from "../api";
import { CommandButton } from "../components/CommandButton";
import { MiningReportCard } from "../components/MiningReportCard";
import { Stat } from "../components/Stat";
import { MineSettingsCard } from "../dict/MineSettingsCard";

interface Props {
  onError: (message: string) => void;
}

/** 实词。助词、符号做成卡片没有意义。 */
const POS_CHOICES = ["NOUN", "PROPN", "VERB", "ADJ", "ADV"];

/** 刚发起一个作业时的进度。真正的数字随 anki://progress 事件更新。 */
function startedProgress(job: string, total: number): AnkiProgress {
  return {
    job,
    done: 0,
    total,
    lemma: "",
    outcome: "",
    newSentences: 0,
    message: "",
    finished: false,
    cancelled: false,
    error: "",
  };
}

export function AnkiPage({ onError }: Props) {
  const [status, setStatus] = useState<AnkiStatus | null>(null);
  const [words, setWords] = useState<WordCandidate[]>([]);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [deck, setDeck] = useState("");
  const [maxExamples, setMaxExamples] = useState(2);
  const [maxDicts, setMaxDicts] = useState(3);
  const [skipStudied, setSkipStudied] = useState(true);
  const [pos, setPos] = useState<string[]>(POS_CHOICES);
  const [preview, setPreview] = useState<AnkiCard | null>(null);
  const [progress, setProgress] = useState<AnkiProgress | null>(null);
  const [running, setRunning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [log, setLog] = useState<AnkiProgress[]>([]);
  const [dupMode, setDupMode] = useState<AnkiDupMode>("append");
  const [dupScope, setDupScope] = useState<AnkiDupScope>("deck");
  const [audioAvailable, setAudioAvailable] = useState(false);
  const [clipAudio, setClipAudio] = useState(true);
  const [refreshScope, setRefreshScope] = useState<AnkiRefreshScope>("deck");
  /** 范围内的旧卡数；null 表示还没数 */
  const [refreshCount, setRefreshCount] = useState<number | null>(null);
  const [counting, setCounting] = useState(false);
  const errorRef = useRef(onError);
  errorRef.current = onError;

  const loadWords = useCallback(
    async (posFilter: string[], skip: boolean) => {
      try {
        setWords(
          await api.ankiWords({ pos: posFilter, limit: 300, skipStudied: skip }),
        );
      } catch (e) {
        errorRef.current(String((e as { message?: string })?.message ?? e));
      }
    },
    [],
  );

  useEffect(() => {
    api
      .ankiStatus()
      .then((s) => {
        setStatus(s);
        // 默认选一个看起来像日语学习的牌组，省得每次都要挑
        if (s.decks.length > 0) {
          setDeck(
            s.decks.find((d) => /日本|jp|japanese|語/i.test(d)) ?? s.decks[0] ?? "",
          );
        }
      })
      .catch(() => undefined);
    void api.ankiIsRunning().then(setRunning).catch(() => undefined);
    void api.ankiAudioAvailable().then(setAudioAvailable).catch(() => undefined);
  }, []);

  useEffect(() => {
    void loadWords(pos, skipStudied);
  }, [pos, skipStudied, loadWords]);

  // 换了牌组或范围，之前数的就不作数了
  useEffect(() => setRefreshCount(null), [deck, refreshScope]);

  useEffect(() => {
    const unlisten = listen<AnkiProgress>("anki://progress", (event) => {
      const payload = event.payload;
      setProgress(payload);
      if (!payload.finished && payload.lemma !== "") {
        setLog((prev) => [payload, ...prev].slice(0, 200));
      }
      if (payload.finished) {
        setRunning(false);
        if (payload.error !== "") errorRef.current(payload.error);
        void api.ankiStatus().then(setStatus).catch(() => undefined);
        void loadWords(pos, skipStudied);
      }
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [pos, skipStudied, loadWords]);

  const toggle = useCallback((lemma: string) => {
    setPicked((prev) => {
      const next = new Set(prev);
      if (!next.delete(lemma)) next.add(lemma);
      return next;
    });
  }, []);

  const showPreview = useCallback(
    async (word: WordCandidate) => {
      try {
        setPreview(
          await api.ankiPreview(word.lemma, word.pos, maxExamples, maxDicts),
        );
      } catch (e) {
        errorRef.current(String((e as { message?: string })?.message ?? e));
      }
    },
    [maxExamples, maxDicts],
  );

  const start = useCallback(async () => {
    const selected = words.filter((w) => picked.has(w.lemma));
    if (selected.length === 0) {
      errorRef.current("先勾几个词");
      return;
    }
    setBusy(true);
    setLog([]);
    try {
      const total = await api.ankiExportStart(
        selected.map((w) => ({ lemma: w.lemma, pos: w.pos })),
        deck,
        maxExamples,
        maxDicts,
        { dupMode, dupScope, clipAudio: audioAvailable && clipAudio },
      );
      setRunning(true);
      setProgress(startedProgress("export", total));
    } catch (e) {
      errorRef.current(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(false);
    }
  }, [words, picked, deck, maxExamples, maxDicts, dupMode, dupScope, audioAvailable, clipAudio]);

  const startUpdate = useCallback(async () => {
    const selected = words.filter((w) => picked.has(w.lemma));
    if (selected.length === 0) {
      errorRef.current("先勾几个词");
      return;
    }
    setBusy(true);
    setLog([]);
    try {
      const total = await api.ankiUpdateStart(
        selected.map((w) => ({ lemma: w.lemma, pos: w.pos })),
        deck,
        dupScope,
      );
      setRunning(true);
      setProgress(startedProgress("update", total));
    } catch (e) {
      errorRef.current(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(false);
    }
  }, [words, picked, deck, dupScope]);

  const countRefresh = useCallback(async () => {
    setCounting(true);
    try {
      setRefreshCount(await api.ankiRefreshPreview(deck, refreshScope));
    } catch (e) {
      errorRef.current(String((e as { message?: string })?.message ?? e));
    } finally {
      setCounting(false);
    }
  }, [deck, refreshScope]);

  const startRefresh = useCallback(async () => {
    setBusy(true);
    setLog([]);
    try {
      await api.ankiRefreshStart(deck, refreshScope);
      setRunning(true);
      setProgress(startedProgress("refresh", refreshCount ?? 0));
      setRefreshCount(null);
    } catch (e) {
      errorRef.current(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(false);
    }
  }, [deck, refreshScope, refreshCount]);

  const pct = useMemo(
    () => (progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0),
    [progress],
  );
  const counts = useMemo(() => {
    const out: Record<string, number> = {};
    for (const entry of log) out[entry.outcome] = (out[entry.outcome] ?? 0) + 1;
    return out;
  }, [log]);

  return (
    <div className="page tools">
      <p className="page-lead">
        把语料里的词做成卡片。例句取自你自己听的歌——
        「夜」的例句是《夜に駆ける》里的那一句，不是教科书造的。
      </p>

      <div className="card">
        <div className="card-head">
          <h2>连接</h2>
          {status !== null && (
            <span className="muted small">
              {status.connected ? `AnkiConnect v${status.version}` : "未连接"}
            </span>
          )}
        </div>
        {status !== null && (
          <>
            <div className="stats">
              <Stat label="牌组" value={status.decks.length} />
              <Stat label="已进 Anki 的词" value={status.knownWords} />
              <Stat label="其中复习过" value={status.studiedWords} />
            </div>
            {!status.connected && (
              <p className="warn">
                {status.message}
                <br />
                选词和预览不需要 Anki——那些只读本地文件。只有真正推卡片
                才需要它开着。
              </p>
            )}
            {status.collectionPath !== "" && (
              <p className="muted small">学习状态读自 {status.collectionPath}</p>
            )}
            {status.connected && !status.noteTypeReady && (
              <p className="muted small">
                还没有「JPOP Corpus」笔记类型，第一次导出时会自动建。
              </p>
            )}
            {status.connected && (
              <div className="toolbar">
                <label className="check">
                  牌组
                  <select value={deck} onChange={(e) => setDeck(e.target.value)}>
                    {status.decks.map((d) => (
                      <option key={d} value={d}>
                        {d}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
            )}
          </>
        )}
      </div>

      <MineSettingsCard connected={status?.connected ?? false} decks={status?.decks ?? []} title="一键制卡" />

      <MiningReportCard onError={onError} />

      <div className="card">
        <div className="card-head">
          <h2>选词</h2>
          <span className="muted small">
            {picked.size} / {words.length} 已选
          </span>
        </div>
        <div className="chips">
          {POS_CHOICES.map((p) => (
            <button
              key={p}
              className={`chip${pos.includes(p) ? " on" : ""}`}
              onClick={() =>
                setPos((prev) =>
                  prev.includes(p) ? prev.filter((x) => x !== p) : [...prev, p],
                )
              }
            >
              {POS_LABELS[p] ?? p}
            </button>
          ))}
          <button
            className={`chip${skipStudied ? " on" : ""}`}
            onClick={() => setSkipStudied((v) => !v)}
          >
            藏起已学过的
          </button>
        </div>

        <div className="word-picker">
          {words.map((w) => (
            <div className={`pick-row${picked.has(w.lemma) ? " on" : ""}`} key={w.lemma}>
              <label className="check">
                <input
                  type="checkbox"
                  checked={picked.has(w.lemma)}
                  onChange={() => toggle(w.lemma)}
                />
                <b>{w.lemma}</b>
              </label>
              <span className="muted small">
                {POS_LABELS[w.pos] ?? w.pos} · {w.count} 次 · {w.songCount} 首
                {w.studied ? " · 已学过" : w.inAnki ? " · 已在 Anki" : ""}
              </span>
              <button className="chip" onClick={() => void showPreview(w)}>
                预览
              </button>
            </div>
          ))}
          {words.length === 0 && (
            <p className="muted small">没有符合条件的词。</p>
          )}
        </div>

        <div className="toolbar">
          <CommandButton icon="check" label="全选" onClick={() => setPicked(new Set(words.map((w) => w.lemma)))} />
          <CommandButton icon="clear" label="清空" onClick={() => setPicked(new Set())} />
          <label className="check">
            每张卡例句
            <input
              type="number"
              min={1}
              max={8}
              value={maxExamples}
              onChange={(e) => setMaxExamples(Number(e.target.value))}
              style={{ width: 52 }}
            />
          </label>
          <label className="check">
            释义取前几部词典
            <input
              type="number"
              min={0}
              max={20}
              value={maxDicts}
              onChange={(e) => setMaxDicts(Number(e.target.value))}
              style={{ width: 52 }}
            />
          </label>
        </div>
        <p className="muted small">
          库里启用了 19 部词典，全上的话一个常用词的释义能到 17,000 字符，
          卡片背面读不完。取前几部就够——顺序就是你在词典管理里排的优先级。
        </p>
      </div>

      {preview !== null && (
        <div className="card">
          <div className="card-head">
            <h2>预览 · {preview.expression}</h2>
            <button className="chip" onClick={() => setPreview(null)}>
              收起
            </button>
          </div>
          <div className="track-meta">
            <span>{preview.reading || "—"}</span>
            <span>{preview.partOfSpeech || "—"}</span>
            {preview.jlpt !== "" && <span>{preview.jlpt}</span>}
            {preview.pitch !== "" && <span>{preview.pitch}</span>}
            {preview.freq !== "" && <span>JPDB {preview.freq}</span>}
          </div>
          {preview.noExamples ? (
            <p className="warn">语料里没有这个词的例句，这张卡做出来没法用。</p>
          ) : (
            <div
              className="card-preview"
              dangerouslySetInnerHTML={{ __html: preview.sentence }}
            />
          )}
          {preview.noDefinitions ? (
            <p className="muted small">本地词典里查不到释义。</p>
          ) : (
            <details>
              <summary className="muted small">
                释义（{preview.meaning.length.toLocaleString()} 字符）
              </summary>
              <div
                className="card-preview"
                dangerouslySetInnerHTML={{ __html: preview.meaning }}
              />
            </details>
          )}
        </div>
      )}

      <div className="card">
        <div className="card-head">
          <h2>导出</h2>
        </div>
        <div className="toolbar">
          <label
            className="check"
            title={
              audioAvailable
                ? "用 ffmpeg 从原曲里切出例句那一句，放进卡片"
                : "找不到 ffmpeg（PATH 或项目根目录）"
            }
          >
            <input
              type="checkbox"
              checked={audioAvailable && clipAudio}
              disabled={!audioAvailable}
              onChange={(e) => setClipAudio(e.target.checked)}
            />
            音频片段
          </label>
          <label className="check">
            已有这个词时
            <select value={dupMode} onChange={(e) => setDupMode(e.target.value as AnkiDupMode)}>
              <option value="append">追加新例句</option>
              <option value="skip">跳过</option>
            </select>
          </label>
          <label className="check">
            查重范围
            <select value={dupScope} onChange={(e) => setDupScope(e.target.value as AnkiDupScope)}>
              <option value="deck">当前牌组</option>
              <option value="root">主牌组及子牌组</option>
            </select>
          </label>
        </div>
        {running && progress !== null && (
          <>
            <div className="seekbar" style={{ margin: "10px 0 6px" }}>
              <div style={{ width: `${pct}%` }} />
            </div>
            <p className="muted small">
              {progress.done}/{progress.total}
              {progress.lemma !== "" ? ` · ${progress.lemma}` : ""}
            </p>
          </>
        )}
        {progress?.finished && (
          <p className="muted small">
            {progress.cancelled ? "已中断。" : "跑完了。"}
            {Object.entries(counts)
              .map(([k, n]) => `${ANKI_OUTCOME_LABELS[k] ?? k} ${n}`)
              .join("，")}
          </p>
        )}
        <div className="toolbar">
          <button
            onClick={() => void start()}
            disabled={busy || running || picked.size === 0 || status?.connected !== true}
          >
            {running ? "处理中…" : `导出 ${picked.size} 张`}
          </button>
          <button
            onClick={() => void startUpdate()}
            disabled={busy || running || picked.size === 0 || status?.connected !== true}
            title="只重查读音、释义、JLPT、音高、词频、词性，不动例句、音频和出处"
          >
            更新选中的 {picked.size} 张
          </button>
          <button onClick={() => void api.ankiCancel()} disabled={!running}>
            中断
          </button>
        </div>
        {status?.connected !== true && (
          <p className="muted small">Anki 没开，导出按钮不可用。</p>
        )}

        {log.length > 0 && (
          <div className="list">
            {log.slice(0, 30).map((entry, i) => (
              <p className="muted small" key={`${entry.lemma}-${i}`}>
                {entry.lemma} · {ANKI_OUTCOME_LABELS[entry.outcome] ?? entry.outcome}
                {entry.outcome === "updated" ? `（+${entry.newSentences} 句）` : ""}
                {entry.message !== "" ? ` · ${entry.message}` : ""}
              </p>
            ))}
          </div>
        )}
      </div>

      <div className="card">
        <div className="card-head">
          <h2>刷新旧牌组</h2>
          <span className="muted small">{deck || "先在上面选牌组"}</span>
        </div>
        <p className="muted small">
          按现在的词典重写范围内所有 JPOP Corpus 卡片的读音、释义、JLPT、音高、词频、词性，
          顺带更新卡片模板。<b>不动例句、音频和出处。</b>
          释义不按上面的「取前几部词典」截断——已有的卡是按全量做的，截断的话刷一次所有卡都会变短。
        </p>
        <div className="toolbar">
          <label className="check">
            范围
            <select
              value={refreshScope}
              onChange={(e) => setRefreshScope(e.target.value as AnkiRefreshScope)}
            >
              <option value="deck">仅当前牌组</option>
              <option value="deckAndChildren">当前牌组及子牌组</option>
              <option value="rootAndChildren">主牌组及子牌组</option>
            </select>
          </label>
          {refreshCount === null ? (
            <button
              onClick={() => void countRefresh()}
              disabled={counting || running || !deck || status?.connected !== true}
            >
              {counting ? "统计中…" : "先数一下有几张"}
            </button>
          ) : (
            <button
              onClick={() => void startRefresh()}
              disabled={busy || running || refreshCount === 0}
            >
              {refreshCount === 0 ? "范围内没有旧卡" : `刷新这 ${refreshCount} 张`}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
