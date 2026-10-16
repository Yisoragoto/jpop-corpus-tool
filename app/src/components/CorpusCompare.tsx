/**
 * 分析页的两张卡：副歌对词频的影响、子语料对比。
 *
 * 计算全在 Rust（`jp-corpus/src/analysis.rs`），这里只管选范围和展示。
 * 每个结果下面都写出处：哪些歌、多少词、按什么单位数的、库指纹——
 * 指纹变了（导入了新歌、校正了分词）就说明结果是旧数据上算的，要重算。
 *
 * 每个词都能点「检索」去 KWIC 看具体是哪几句、点 ▶ 听；分析不是终点。
 */

import { useEffect, useMemo, useState } from "react";

import {
  api,
  POS_LABELS,
  type CountUnit,
  type FrequencyResult,
  type KeyItem,
  type KeynessResult,
  type Provenance,
  type Subcorpus,
} from "../api";
import { PerformerPicker } from "./PerformerPicker";

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

export function ProvenanceLine({ p }: { p: Provenance }) {
  return (
    <p
      className="provenance muted small"
      title={`规则 ${p.rules} · 库指纹 ${p.snapshot.fingerprint} · 结构版本 ${p.snapshot.schemaVersion}`}
    >
      {p.scope} · {p.songs} 首 / {p.tokens.toLocaleString()} 词（
      {p.unit === "distinctLines" ? "重复行只算一次" : "全部词次"}）· 指纹 {p.snapshot.fingerprint.slice(0, 8)}
    </p>
  );
}

// ────────────────────────────── 副歌效应 ──────────────────────────────

export function RepeatEffectCard({
  performerIds,
  jpOnly,
  onSearchWord,
  onError,
}: {
  performerIds: number[];
  jpOnly: boolean;
  onSearchWord: (lemma: string) => void;
  onError: (message: string) => void;
}) {
  const [raw, setRaw] = useState<FrequencyResult | null>(null);
  const [distinct, setDistinct] = useState<FrequencyResult | null>(null);

  useEffect(() => {
    let cancelled = false;
    const subcorpus: Subcorpus = { performerIds };
    const run = (unit: CountUnit) =>
      api.corpusFrequency({ scope: { subcorpus, unit, jpOnly }, pos: null, limit: 100_000 });
    Promise.all([run("tokens"), run("distinctLines")])
      .then(([a, b]) => {
        if (cancelled) return;
        setRaw(a);
        setDistinct(b);
      })
      .catch((e) => !cancelled && onError(message(e)));
    return () => {
      cancelled = true;
    };
  }, [performerIds, jpOnly, onError]);

  const inflated = useMemo(
    () =>
      (raw?.items ?? [])
        // 只在一首歌里出现的词（英文副歌、口癖）重复占比天然接近 100%，说明不了什么，至少要两首
        .filter((i) => i.rawFreq >= 10 && i.songs >= 2 && i.pos !== "AUX" && i.pos !== "ADP")
        .sort((a, b) => b.repeatShare - a.repeatShare || b.rawFreq - a.rawFreq)
        .slice(0, 12),
    [raw],
  );

  if (!raw || !distinct) return null;
  const share = raw.provenance.tokens > 0 ? 1 - distinct.provenance.tokens / raw.provenance.tokens : 0;

  return (
    <section className="card">
      <div className="card-head">
        <h2>副歌对词频的影响</h2>
      </div>
      <p className="muted small">
        歌词里的副歌会原样唱好几遍。同一首歌里重复出现的行占了全部词次的{" "}
        <strong>{(share * 100).toFixed(1)}%</strong>（{raw.provenance.tokens.toLocaleString()} →{" "}
        {distinct.provenance.tokens.toLocaleString()} 词）。下面这些词的频次大半来自重复行：
        按「唱了几遍」数和按「写了几次」数，排名会很不一样。
      </p>
      <table className="data">
        <thead>
          <tr>
            <th>词元</th>
            <th>词性</th>
            <th className="num" title="唱几遍算几遍">全部词次</th>
            <th className="num" title="同一首歌里重复的行只算一次">去重后</th>
            <th className="num">来自重复行</th>
            <th className="num">曲数</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {inflated.map((w) => (
            <tr key={`${w.lemma}:${w.pos}`}>
              <td>{w.lemma}</td>
              <td className="muted">{POS_LABELS[w.pos] ?? w.pos}</td>
              <td className="num">{w.rawFreq}</td>
              <td className="num">{w.distinctFreq}</td>
              <td className="num">
                <span className="share-bar" style={{ ["--share" as string]: `${w.repeatShare * 100}%` }}>
                  {(w.repeatShare * 100).toFixed(0)}%
                </span>
              </td>
              <td className="num">{w.songs}</td>
              <td className="row-actions">
                <button className="link-btn" onClick={() => onSearchWord(w.lemma)}>
                  检索
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <ProvenanceLine p={raw.provenance} />
    </section>
  );
}

// ────────────────────────────── 子语料对比 ──────────────────────────────

interface SideState {
  performerIds: number[];
  yearFrom: string;
  yearTo: string;
}

const EMPTY_SIDE: SideState = { performerIds: [], yearFrom: "", yearTo: "" };

function toSubcorpus(side: SideState): Subcorpus {
  const year = (s: string) => (/^\d{4}$/.test(s.trim()) ? Number(s.trim()) : null);
  return { performerIds: side.performerIds, yearFrom: year(side.yearFrom), yearTo: year(side.yearTo) };
}

function isEmpty(side: SideState) {
  const s = toSubcorpus(side);
  return !s.performerIds?.length && s.yearFrom == null && s.yearTo == null;
}

function SidePicker({
  label,
  side,
  onChange,
  onError,
}: {
  label: string;
  side: SideState;
  onChange: (s: SideState) => void;
  onError: (m: string) => void;
}) {
  return (
    <div className="compare-side">
      <span className="muted small compare-label">{label}</span>
      <PerformerPicker
        selected={side.performerIds}
        onChange={(performerIds) => onChange({ ...side, performerIds })}
        onError={onError}
      />
      <span className="year-range">
        <input
          className="year-input"
          inputMode="numeric"
          placeholder="年份"
          aria-label={`${label}起始年份`}
          value={side.yearFrom}
          onChange={(e) => onChange({ ...side, yearFrom: e.target.value })}
        />
        –
        <input
          className="year-input"
          inputMode="numeric"
          placeholder="年份"
          aria-label={`${label}结束年份`}
          value={side.yearTo}
          onChange={(e) => onChange({ ...side, yearTo: e.target.value })}
        />
      </span>
    </div>
  );
}

function KeyTable({ title, items, onSearchWord }: { title: string; items: KeyItem[]; onSearchWord: (l: string) => void }) {
  return (
    <div className="key-col">
      <h3>{title}</h3>
      {items.length === 0 && <p className="muted small">没有显著的词</p>}
      {items.length > 0 && (
        <table className="data">
          <thead>
            <tr>
              <th>词元</th>
              <th className="num" title="每万词">目标</th>
              <th className="num" title="每万词">参照</th>
              <th className="num" title="对数似然（显著性）">LL</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {items.map((k) => (
              <tr key={`${k.lemma}:${k.pos}`} title={`${POS_LABELS[k.pos] ?? k.pos} · 目标 ${k.targetFreq} 次 / 参照 ${k.referenceFreq} 次 · log ratio ${k.logRatio.toFixed(2)}`}>
                <td>
                  {k.lemma} <span className="muted small">{POS_LABELS[k.pos] ?? k.pos}</span>
                </td>
                <td className="num">{k.targetPer10k.toFixed(1)}</td>
                <td className="num">{k.referencePer10k.toFixed(1)}</td>
                <td className="num">{Math.abs(k.logLikelihood).toFixed(1)}</td>
                <td className="row-actions">
                  <button className="link-btn" onClick={() => onSearchWord(k.lemma)}>
                    检索
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

export function KeynessCard({
  jpOnly,
  onSearchWord,
  onError,
}: {
  jpOnly: boolean;
  onSearchWord: (lemma: string) => void;
  onError: (message: string) => void;
}) {
  const [target, setTarget] = useState<SideState>({ ...EMPTY_SIDE, yearFrom: "2020" });
  const [reference, setReference] = useState<SideState>(EMPTY_SIDE);
  const [unit, setUnit] = useState<CountUnit>("distinctLines");
  const [result, setResult] = useState<KeynessResult | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async () => {
    if (isEmpty(target)) {
      onError("先给「目标」选一个范围（歌手或年份）");
      return;
    }
    setBusy(true);
    try {
      setResult(
        await api.keyness({
          target: toSubcorpus(target),
          reference: isEmpty(reference) ? null : toSubcorpus(reference),
          unit,
          jpOnly,
          minFreq: 5,
          limit: 25,
        }),
      );
    } catch (e) {
      onError(message(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="card">
      <div className="card-head">
        <h2>子语料对比</h2>
      </div>
      <p className="muted small">
        选两批歌（按歌手、年份），看哪些词在「目标」里明显偏多、偏少。参照不选就是语料库里其余的歌。
        按对数似然排序，两边合计少于 5 次的词不列出。
      </p>
      <div className="compare-form">
        <SidePicker label="目标" side={target} onChange={setTarget} onError={onError} />
        <SidePicker label="参照" side={reference} onChange={setReference} onError={onError} />
        <div className="compare-side">
          <div className="segmented">
            <button
              className={unit === "distinctLines" ? "on" : ""}
              onClick={() => setUnit("distinctLines")}
              title="同一首歌里重复的行只算一次"
            >
              去掉重复行
            </button>
            <button className={unit === "tokens" ? "on" : ""} onClick={() => setUnit("tokens")} title="唱几遍算几遍">
              全部词次
            </button>
          </div>
          <button className="primary" disabled={busy} onClick={() => void run()}>
            {busy ? "计算中…" : "对比"}
          </button>
        </div>
      </div>
      {result && (
        <>
          <div className="key-cols">
            <KeyTable title="目标里偏多" items={result.over} onSearchWord={onSearchWord} />
            <KeyTable title="目标里偏少" items={result.under} onSearchWord={onSearchWord} />
          </div>
          <p className="muted small">目标</p>
          <ProvenanceLine p={result.target} />
          <p className="muted small">参照</p>
          <ProvenanceLine p={result.reference} />
        </>
      )}
    </section>
  );
}
