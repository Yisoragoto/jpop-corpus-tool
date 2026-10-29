/**
 * 分析页的两张卡：副歌对词频的影响、子语料对比。
 *
 * 计算全在 Rust（`jp-corpus/src/analysis.rs`），这里只管选范围和展示。
 * 每个结果下面都写出处：哪些歌、多少词、按什么单位数的、库指纹——
 * 指纹变了（导入了新歌、校正了分词）就说明结果是旧数据上算的，要重算。
 *
 * 每个词都能点「检索」去 KWIC 看具体是哪几句、点 ▶ 听；分析不是终点。
 *
 * 子语料对比的做法照现在关键词分析的通行建议：先用显著性把「可能只是碰巧」的词筛掉
 * （一次检验几千个词，默认做 Bonferroni 校正），再按效应量（Log Ratio）或对数似然排。
 * 对数似然只说明「有没有差别」，Log Ratio 才说明「差多少」（差 1 = 相对频率差一倍）。
 */

import { useEffect, useMemo, useState } from "react";

import {
  api,
  POS_LABELS,
  type CountUnit,
  type FrequencyResult,
  type KeyItem,
  type KeynessQuery,
  type KeynessResult,
  type Provenance,
  type Subcorpus,
} from "../api";
import { ANALYSIS_ARCHIVED, isWholeCorpus, qualityApi, type AnalysisSpec } from "../corpusQuality";
import { mergeSubcorpus, useSubcorpora, type ActiveScope } from "../subcorpusScope";
import { PerformerPicker } from "./PerformerPicker";
import { tr } from "../i18n";

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

/**
 * 把这次分析存进「分析记录」：连同范围里每首歌的歌词版本、分词器、库指纹一起，之后能原样重跑对比
 * （`jp-corpus/src/runs.rs`）。这里的 Provenance 只说明「是不是在现在的库上算的」，存档回答「为什么变了」。
 */
function ArchiveButton({ spec, label, onError }: { spec: AnalysisSpec; label: string; onError: (m: string) => void }) {
  const [state, setState] = useState<"idle" | "busy" | "done">("idle");
  useEffect(() => setState("idle"), [JSON.stringify(spec)]); // eslint-disable-line
  return (
    <button
      className="ghost"
      disabled={state !== "idle"}
      title={tr("存进下面的「分析记录」，之后可以原样重跑、对比")}
      onClick={async () => {
        setState("busy");
        try {
          await qualityApi.runAnalysis(spec, { performerIds: [], jpOnly: false }, label);
          setState("done");
          window.dispatchEvent(new Event(ANALYSIS_ARCHIVED));
        } catch (e) {
          onError(message(e));
          setState("idle");
        }
      }}
    >
      {state === "done" ? tr("已存档") : state === "busy" ? tr("存档中…") : tr("存档")}
    </button>
  );
}

export function ProvenanceLine({ p }: { p: Provenance }) {
  return (
    <p
      className="provenance muted small"
      title={tr("规则 {rules} · 库指纹 {fingerprint} · 结构版本 {schemaVersion}", { rules: p.rules, fingerprint: p.snapshot.fingerprint, schemaVersion: p.snapshot.schemaVersion })}
    >{tr("{scope} · {songs} 首 / {tokens} 词（", { scope: p.scope, songs: p.songs, tokens: p.tokens.toLocaleString() })}{p.unit === "distinctLines" ? tr("重复行只算一次") : tr("全部词次")}{tr("）· 指纹 {slice}", { slice: p.snapshot.fingerprint.slice(0, 8) })}</p>
  );
}

// ────────────────────────────── 副歌效应 ──────────────────────────────

export function RepeatEffectCard({
  subcorpus,
  jpOnly,
  onSearchWord,
  onError,
}: {
  /** 统计页筛选条里的范围（歌手、年代、流派、语言、完整度……） */
  subcorpus: Subcorpus;
  jpOnly: boolean;
  onSearchWord: (lemma: string) => void;
  onError: (message: string) => void;
}) {
  const [raw, setRaw] = useState<FrequencyResult | null>(null);
  const [distinct, setDistinct] = useState<FrequencyResult | null>(null);
  // 对象每次渲染都是新的，按内容判断变没变
  const scopeKey = JSON.stringify(subcorpus);

  useEffect(() => {
    let cancelled = false;
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
    // scopeKey 就是 subcorpus 的内容（对象每次渲染都是新的）
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scopeKey, jpOnly, onError]);

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
        <h2>{tr("副歌对词频的影响")}</h2>
        <div className="card-tools">
          <ArchiveButton
            spec={{ kind: "corpusFrequency", query: { scope: { subcorpus, unit: "distinctLines", jpOnly }, pos: null, limit: 500 } }}
            label={tr("副歌效应（去重词频）")}
            onError={onError}
          />
        </div>
      </div>
      <p className="muted small">{tr("歌词里的副歌会原样唱好几遍。同一首歌里重复出现的行占了全部词次的{v}", { v: " " })}<strong>{(share * 100).toFixed(1)}%</strong>{tr("（{tokens} →{v}{tokens2} 词）。下面这些词的频次大半来自重复行： 按「唱了几遍」数和按「写了几次」数，排名会很不一样。", { tokens: raw.provenance.tokens.toLocaleString(), v: " ", tokens2: distinct.provenance.tokens.toLocaleString() })}</p>
      <table className="data">
        <thead>
          <tr>
            <th>{tr("词元")}</th>
            <th>{tr("词性")}</th>
            <th className="num" title={tr("唱几遍算几遍")}>{tr("全部词次")}</th>
            <th className="num" title={tr("同一首歌里重复的行只算一次")}>{tr("去重后")}</th>
            <th className="num">{tr("来自重复行")}</th>
            <th className="num">{tr("曲数")}</th>
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
                <button className="link-btn" onClick={() => onSearchWord(w.lemma)}>{tr("检索")}</button>
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
  /** 存过的子语料（「分析」页筛选条里存的）。歌手、年份填了的以这里为准 */
  saved: ActiveScope | null;
}

const EMPTY_SIDE: SideState = { performerIds: [], yearFrom: "", yearTo: "", saved: null };

/** 显著性门槛 */
type Significance = "none" | "0.05" | "0.01" | "0.001";
/** 每边显示几个词 */
const KEY_SHOWN = 25;

function formatP(p: number): string {
  if (p < 0.0001) return "<.0001";
  return p.toFixed(4).replace(/^0/, "");
}

function toSubcorpus(side: SideState): Subcorpus {
  const year = (s: string) => (/^\d{4}$/.test(s.trim()) ? Number(s.trim()) : null);
  return mergeSubcorpus({ performerIds: side.performerIds, yearFrom: year(side.yearFrom), yearTo: year(side.yearTo) }, side.saved);
}

function isEmpty(side: SideState) {
  const s = toSubcorpus(side);
  return isWholeCorpus({ ...s, performerIds: s.performerIds ?? [], jpOnly: false });
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
  const saved = useSubcorpora(onError);
  return (
    <div className="compare-side">
      <span className="muted small compare-label">{label}</span>
      {saved.length > 0 && (
        <select
          className="rate subcorpus-picker"
          value={side.saved?.id ?? ""}
          onChange={(e) => {
            const pick = saved.find((x) => String(x.id) === e.target.value);
            onChange({ ...side, saved: pick ? { id: pick.id, name: pick.name, filter: pick.filter } : null });
          }}
          title={tr("存过的子语料")}
        >
          <option value="">{tr("（不用子语料）")}</option>
          {saved.map((x) => (
            <option key={x.id} value={x.id}>
              {x.name}
            </option>
          ))}
        </select>
      )}
      <PerformerPicker
        selected={side.performerIds}
        onChange={(performerIds) => onChange({ ...side, performerIds })}
        onError={onError}
      />
      <span className="year-range">
        <input
          className="year-input"
          inputMode="numeric"
          placeholder={tr("年份")}
          aria-label={tr("{label}起始年份", { label })}
          value={side.yearFrom}
          onChange={(e) => onChange({ ...side, yearFrom: e.target.value })}
        />
        –
        <input
          className="year-input"
          inputMode="numeric"
          placeholder={tr("年份")}
          aria-label={tr("{label}结束年份", { label })}
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
      {items.length === 0 && <p className="muted small">{tr("没有显著的词")}</p>}
      {items.length > 0 && (
        <table className="data">
          <thead>
            <tr>
              <th>{tr("词元")}</th>
              <th className="num" title={tr("每万词")}>{tr("目标")}</th>
              <th className="num" title={tr("每万词")}>{tr("参照")}</th>
              <th className="num" title={tr("对数似然（显著性）")}>LL</th>
              <th className="num" title={tr("Log Ratio（效应量）：log₂(目标相对频率 ÷ 参照相对频率)。差 1 就是相差一倍")}>LR</th>
              <th className="num" title={tr("未校正的 p 值")}>p</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {items.map((k) => (
              <tr
                key={`${k.lemma}:${k.pos}`}
                title={tr("{v} · 目标 {targetFreq} 次 / 参照 {referenceFreq} 次 · %DIFF {diff} · BIC {bic}", {
                  v: POS_LABELS[k.pos] ?? k.pos,
                  targetFreq: k.targetFreq,
                  referenceFreq: k.referenceFreq,
                  diff: k.percentDiff === null ? "∞" : `${k.percentDiff.toFixed(0)}%`,
                  bic: k.bic.toFixed(1),
                })}
              >
                <td>
                  {k.lemma} <span className="muted small">{POS_LABELS[k.pos] ?? k.pos}</span>
                </td>
                <td className="num">{k.targetPer10k.toFixed(1)}</td>
                <td className="num">{k.referencePer10k.toFixed(1)}</td>
                <td className="num">{Math.abs(k.logLikelihood).toFixed(1)}</td>
                <td className="num">{k.logRatio.toFixed(2)}</td>
                <td className="num muted">{formatP(k.pValue)}</td>
                <td className="row-actions">
                  <button className="link-btn" onClick={() => onSearchWord(k.lemma)}>{tr("检索")}</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

/** 每边取前 KEY_SHOWN 个：按对数似然（后端已经排好）或按 Log Ratio 的绝对值 */
function rank(items: KeyItem[], by: "ll" | "lr"): KeyItem[] {
  const sorted = by === "ll" ? items : [...items].sort((a, b) => Math.abs(b.logRatio) - Math.abs(a.logRatio) || Math.abs(b.logLikelihood) - Math.abs(a.logLikelihood));
  return sorted.slice(0, KEY_SHOWN);
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
  const [significance, setSignificance] = useState<Significance>("0.05");
  const [bonferroni, setBonferroni] = useState(true);
  const [rankBy, setRankBy] = useState<"ll" | "lr">("ll");
  const [result, setResult] = useState<KeynessResult | null>(null);
  const [lastQuery, setLastQuery] = useState<KeynessQuery | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async () => {
    if (isEmpty(target)) {
      onError(tr("先给「目标」选一个范围（歌手或年份）"));
      return;
    }
    setBusy(true);
    try {
      const query: KeynessQuery = {
        target: toSubcorpus(target),
        reference: isEmpty(reference) ? null : toSubcorpus(reference),
        unit,
        jpOnly,
        minFreq: 5,
        // 多取一些：按 Log Ratio 排时要从全部显著的词里挑，不是从对数似然前 25 个里挑
        limit: 1000,
        maxP: significance === "none" ? null : Number(significance),
        bonferroni: significance !== "none" && bonferroni,
      };
      setResult(await api.keyness(query));
      setLastQuery(query);
    } catch (e) {
      onError(message(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="card">
      <div className="card-head">
        <h2>{tr("子语料对比")}</h2>
        <div className="card-tools">
          {result && lastQuery && <ArchiveButton spec={{ kind: "keyness", query: lastQuery }} label={tr("子语料对比")} onError={onError} />}
        </div>
      </div>
      <p className="muted small">{tr("选两批歌（存过的子语料，或者按歌手、年份），看哪些词在「目标」里明显偏多、偏少。参照不选就是语料库里其余的歌。 按对数似然排序，两边合计少于 5 次的词不列出。")}{" "}{tr("先按 p 值筛掉可能只是碰巧的词，再按对数似然（差别有多确定）或 Log Ratio（差多少）排。")}</p>
      <div className="compare-form">
        <SidePicker label={tr("目标")} side={target} onChange={setTarget} onError={onError} />
        <SidePicker label={tr("参照")} side={reference} onChange={setReference} onError={onError} />
        <div className="compare-side">
          <div className="segmented">
            <button
              className={unit === "distinctLines" ? "on" : ""}
              onClick={() => setUnit("distinctLines")}
              title={tr("同一首歌里重复的行只算一次")}
            >{tr("去掉重复行")}</button>
            <button className={unit === "tokens" ? "on" : ""} onClick={() => setUnit("tokens")} title={tr("唱几遍算几遍")}>{tr("全部词次")}</button>
          </div>
          <select
            className="rate"
            value={significance}
            onChange={(e) => setSignificance(e.target.value as Significance)}
            title={tr("只列差别显著的词")}
          >
            <option value="none">{tr("不筛显著性")}</option>
            <option value="0.05">p &lt; 0.05</option>
            <option value="0.01">p &lt; 0.01</option>
            <option value="0.001">p &lt; 0.001</option>
          </select>
          <label className="check" title={tr("一次检验几千个词，不校正的话 p < 0.05 里会有不少是碰巧。校正 = 阈值除以检验的词数")}>
            <input type="checkbox" checked={bonferroni} disabled={significance === "none"} onChange={(e) => setBonferroni(e.target.checked)} />
            Bonferroni
          </label>
          <button className="primary" disabled={busy} onClick={() => void run()}>
            {busy ? tr("计算中…") : tr("对比")}
          </button>
        </div>
      </div>
      {result && (
        <>
          <div className="key-meta">
            <span className="muted small">
              {tr("检验了 {tested} 个词", { tested: result.tested.toLocaleString() })}
              {result.pThreshold !== null && tr("，门槛 p ≤ {threshold}", { threshold: result.pThreshold.toPrecision(2) })}
              {tr("；显著偏多 {over}、偏少 {under}", { over: result.over.length, under: result.under.length })}
            </span>
            <div className="segmented small-seg">
              <button className={rankBy === "ll" ? "on" : ""} onClick={() => setRankBy("ll")} title={tr("差别有多确定")}>{tr("按 LL 排")}</button>
              <button className={rankBy === "lr" ? "on" : ""} onClick={() => setRankBy("lr")} title={tr("差多少（效应量）")}>{tr("按 Log Ratio 排")}</button>
            </div>
          </div>
          <div className="key-cols">
            <KeyTable title={tr("目标里偏多")} items={rank(result.over, rankBy)} onSearchWord={onSearchWord} />
            <KeyTable title={tr("目标里偏少")} items={rank(result.under, rankBy)} onSearchWord={onSearchWord} />
          </div>
          <p className="muted small">{tr("目标")}</p>
          <ProvenanceLine p={result.target} />
          <p className="muted small">{tr("参照")}</p>
          <ProvenanceLine p={result.reference} />
        </>
      )}
    </section>
  );
}
