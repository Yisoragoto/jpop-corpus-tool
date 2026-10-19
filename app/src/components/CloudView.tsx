/**
 * 资料库里的词云：占中栏和右栏。筛选（歌手、词性、仅日文）、字体、词数都在卡片头上。
 *
 * **状态记在模块里**（`memory`），不跟着组件走：点一首歌去看歌词、再点返回，
 * 或者切到别的页面再回来，看到的还是刚才那一片云、刚才选中的那个词。
 *
 * 词性默认只看名词：助词、助动词数量最多，全放进来云里就只剩「の・に・て」。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api, POS_LABELS, type FrequencyRow } from "../api";
import { messageOf } from "../errors";
import { useFontOptions, useLyricFontFiles } from "../fonts";
import { updateSettings, useAppSettings } from "../settings";
import { PerformerPicker } from "./PerformerPicker";
import { SubcorpusPicker } from "./SubcorpusPicker";
import { useActiveScope, withScope } from "../subcorpusScope";
import { WordCloud, type NowLine } from "./WordCloud";
import { tr } from "../i18n";

/** 可选的词性。「全部」= 不限 */
export const CLOUD_POS = ["NOUN", "PROPN", "VERB", "ADJ", "ADV"] as const;
const COUNTS = [60, 120, 200] as const;
/** 每种词性取多少行再合并。云里最多两百来个词，取四百够了 */
const PER_POS = 400;

interface Memory {
  performerIds: number[];
  /** 空 = 全部词性 */
  pos: string[];
  jpOnly: boolean;
  picked: string | null;
  /** 全屏开着：从全屏歌词、歌词页回来时还开着 */
  present: boolean;
  /** 发行年份范围，null = 不限 */
  yearFrom: number | null;
  yearTo: number | null;
}

const memory: Memory = { performerIds: [], pos: ["NOUN"], jpOnly: true, picked: null, present: false, yearFrom: null, yearTo: null };

/** 从某个人的档案打开：换成这个人，选中的词清掉（换了人，那个词多半不在了） */
export function focusCloudOn(personId: number) {
  if (memory.performerIds.length === 1 && memory.performerIds[0] === personId) return;
  memory.performerIds = [personId];
  memory.picked = null;
}

interface Props {
  playing: { songId: string; utteranceId: number } | null;
  nowLine: NowLine | null;
  onPlayLine: (songId: string, utteranceId: number, timeSec: number | null, fullscreen: boolean) => void;
  /** 上面盖着全屏歌词 */
  covered: boolean;
  /** 一首歌的封面路径（列表里显示） */
  coverOf: (songId: string) => string;
  onSearchWord: (lemma: string) => void;
  onShowExamples: (lemma: string) => void;
  onError: (message: string) => void;
}

export function CloudView({ playing, nowLine, onPlayLine, covered, coverOf, onSearchWord, onShowExamples, onError }: Props) {
  const settings = useAppSettings();
  const [performerIds, setPerformerIds] = useState(memory.performerIds);
  const [pos, setPos] = useState(memory.pos);
  const [jpOnly, setJpOnly] = useState(memory.jpOnly);
  const [picked, setPicked] = useState(memory.picked);
  const [rows, setRows] = useState<FrequencyRow[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [present, setPresent] = useState(memory.present);
  const [yearFrom, setYearFrom] = useState(memory.yearFrom);
  const [yearTo, setYearTo] = useState(memory.yearTo);
  /** 曲库里有哪些年份（下拉的选项） */
  const [years, setYears] = useState<number[]>([]);
  useEffect(() => {
    let alive = true;
    api
      .listTracks()
      .then((tracks) => {
        const ys = new Set<number>();
        for (const t of tracks) if (/^\d{4}/.test(t.year)) ys.add(Number(t.year.slice(0, 4)));
        if (alive) setYears([...ys].sort((a, b) => a - b));
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);
  const request = useRef(0);
  const scope = useActiveScope();

  // 改了就记下来
  useEffect(() => {
    Object.assign(memory, { performerIds, pos, jpOnly, picked, present, yearFrom, yearTo });
  }, [performerIds, pos, jpOnly, picked, present, yearFrom, yearTo]);

  useEffect(() => {
    const id = ++request.current;
    setLoading(true);
    // 这里选的年份压过子语料里的年份：词云上明明写着 2019–2021，就得是这几年
    const scoped = withScope({ performerIds, jpOnly }, scope);
    const filter = {
      ...scoped,
      yearFrom: yearFrom ?? scoped.yearFrom ?? null,
      yearTo: yearTo ?? scoped.yearTo ?? null,
    };
    const job =
      pos.length === 0
        ? api.statsFrequency(filter, undefined, PER_POS * 2)
        : Promise.all(pos.map((p) => api.statsFrequency(filter, p, PER_POS))).then((lists) => lists.flat());
    job
      .then((result) => id === request.current && setRows(result))
      .catch((err: unknown) => id === request.current && onError(tr("读不出词频：{err}", { err: messageOf(err) })))
      .finally(() => id === request.current && setLoading(false));
  }, [performerIds, pos, jpOnly, scope, yearFrom, yearTo, onError]);

  const togglePos = (p: string) =>
    setPos((cur) => {
      if (p === "") return [];
      const next = cur.includes(p) ? cur.filter((x) => x !== p) : [...cur, p];
      // 一个都不选 = 回到默认的名词，不是全部
      return next.length === 0 ? ["NOUN"] : CLOUD_POS.filter((x) => next.includes(x));
    });

  // 字体：下拉列出自带、导入、本机的；导入的文件当场加载
  const fonts = useFontOptions(true, onError);
  useLyricFontFiles([settings.wordCloudFont], onError);
  const fontChoices = [{ value: "", label: tr("界面字体") }, ...(fonts.options ?? []).map((o) => ({ value: o.value, label: o.label }))];
  if (settings.wordCloudFont !== "" && !fontChoices.some((o) => o.value === settings.wordCloudFont)) {
    fontChoices.push({ value: settings.wordCloudFont, label: settings.wordCloudFont });
  }
  const importFont = useCallback(async () => {
    try {
      const chosen = await openDialog({
        multiple: true,
        title: tr("导入字体"),
        filters: [{ name: tr("字体文件"), extensions: ["ttf", "otf", "ttc"] }],
      });
      const paths = chosen === null ? [] : Array.isArray(chosen) ? chosen : [chosen];
      if (paths.length > 0) fonts.addImported(paths);
    } catch (err) {
      onError(messageOf(err));
    }
  }, [fonts, onError]);

  const toolbar = (
    <div className="wc-filters">
      <PerformerPicker selected={performerIds} onChange={setPerformerIds} onError={onError} />
      <SubcorpusPicker onError={onError} />
      <div className="segmented wc-pos" role="group" aria-label={tr("词性")}>
        <button className={pos.length === 0 ? "on" : ""} onClick={() => togglePos("")}>{tr("全部")}</button>
        {CLOUD_POS.map((p) => (
          <button key={p} className={pos.includes(p) ? "on" : ""} onClick={() => togglePos(p)}>
            {POS_LABELS[p] ?? p}
          </button>
        ))}
      </div>
      <label className="check wc-check" title={tr("只要含假名或汉字的词（去掉英文、数字）")}>
        <input type="checkbox" checked={jpOnly} onChange={(e) => setJpOnly(e.target.checked)} />{tr("仅日文")}</label>
      <label
        className="check wc-check"
        title={tr("去掉单个假名的碎片（僕「ら」、一「つ」）、形式名词（こと・もの・まま・よう）和纯符号")}
      >
        <input
          type="checkbox"
          checked={settings.wordCloudHideFunctionWords}
          onChange={(e) => updateSettings({ wordCloudHideFunctionWords: e.target.checked })}
        />{tr("去虚词")}</label>
      <label className="check wc-check" title={tr("名词、动词、形容词……各一种颜色，云下面有图例")}>
        <input
          type="checkbox"
          checked={settings.wordCloudColorByPos}
          onChange={(e) => updateSettings({ wordCloudColorByPos: e.target.checked })}
        />{tr("按词性上色")}</label>
      {years.length > 1 && (
        <span className="wc-years" title={tr("只看这几年发行的歌")}>
          <select
            className="wc-select"
            value={yearFrom ?? ""}
            onChange={(e) => {
              const v = e.target.value === "" ? null : Number(e.target.value);
              setYearFrom(v);
              if (v !== null && yearTo !== null && yearTo < v) setYearTo(v);
            }}
            aria-label={tr("从哪一年")}
          >
            <option value="">{tr("最早")}</option>
            {years.map((y) => (
              <option key={y} value={y}>
                {y}
              </option>
            ))}
          </select>
          <span aria-hidden="true">–</span>
          <select
            className="wc-select"
            value={yearTo ?? ""}
            onChange={(e) => {
              const v = e.target.value === "" ? null : Number(e.target.value);
              setYearTo(v);
              if (v !== null && yearFrom !== null && yearFrom > v) setYearFrom(v);
            }}
            aria-label={tr("到哪一年")}
          >
            <option value="">{tr("最近")}</option>
            {years.map((y) => (
              <option key={y} value={y}>
                {y}
              </option>
            ))}
          </select>
          {(yearFrom !== null || yearTo !== null) && (
            <button
              className="link-btn"
              onClick={() => {
                setYearFrom(null);
                setYearTo(null);
              }}
              title={tr("不限年份")}
            >
              ×
            </button>
          )}
        </span>
      )}
      {settings.wordCloudHidden.length > 0 && (
        <button
          className="btn wc-btn"
          onClick={() => updateSettings({ wordCloudHidden: [] })}
          title={tr("拿掉的词：{wordCloudHidden}。点一下全部放回来", { wordCloudHidden: settings.wordCloudHidden.join(tr("、")) })}
        >{tr("已隐藏 {length} 个 · 恢复", { length: settings.wordCloudHidden.length })}</button>
      )}
      <select
        className="wc-select"
        value={COUNTS.includes(settings.wordCloudCount as (typeof COUNTS)[number]) ? settings.wordCloudCount : 120}
        onChange={(e) => updateSettings({ wordCloudCount: Number(e.target.value) })}
        title={tr("云里放多少个词")}
      >
        {COUNTS.map((n) => (
          <option key={n} value={n}>{tr("{n} 个词", { n })}</option>
        ))}
      </select>
      <select
        className="wc-select font"
        value={settings.wordCloudFont}
        onChange={(e) => updateSettings({ wordCloudFont: e.target.value })}
        title={tr("词云字体（自带、导入或本机的）")}
      >
        {fontChoices.map((o) => (
          <option key={o.value} value={o.value} style={{ fontFamily: o.value ? `"${o.value}"` : undefined }}>
            {o.label}
          </option>
        ))}
      </select>
      <button className="btn wc-btn" onClick={() => void importFont()} title={tr("导入 ttf / otf / ttc，只记住位置，不复制文件")}>{tr("导入字体…")}</button>
    </div>
  );

  return (
    <div className="cloud-view">
      <WordCloud
        rows={rows}
        performerIds={performerIds}
        years={{ from: yearFrom, to: yearTo }}
        loading={loading}
        toolbar={toolbar}
        pickedLemma={picked}
        onPick={setPicked}
        playing={playing}
        nowLine={nowLine}
        onPlayLine={onPlayLine}
        covered={covered}
        coverOf={coverOf}
        onSearchWord={onSearchWord}
        onShowExamples={onShowExamples}
        present={present}
        onPresent={setPresent}
        onError={onError}
      />
    </div>
  );
}
