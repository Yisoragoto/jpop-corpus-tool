/**
 * 查词结果列表，并注入启用词典自带的 styles.css。
 *
 * 作用域照 Yomitan `display.js` 的 `_getCustomCss` 做：整段样式包进 `[data-dictionary="词典名"] { … }`
 * （CSS 嵌套，WebView2 支持），只作用于那本词典的释义，互不污染，也不碰应用自己的界面。
 *
 * 顶上一行工具条：一键折叠 / 展开这次结果里出现的所有词典（折叠状态记住）。
 * 窄栏（`compact`）默认只显示前几条，免得把下面的内容挤到很远。
 */

import { useEffect, useMemo, useState, type ReactNode } from "react";

import { dictApi, type FindTermsResult, type TermDictionaryEntry } from "./api";
import { setDictionariesCollapsed, useCollapsedDictionaries } from "./collapse";
import { TermEntry } from "./TermEntry";
import "./dict.css";

const STYLE_ELEMENT_ID = "dictionary-styles";
/** 窄栏默认显示几条词条 */
const COMPACT_ENTRY_LIMIT = 3;
let stylesLoaded: Promise<void> | null = null;

/** 词典列表变了（导入、启用、删除）之后调用，下次渲染重新取样式。 */
export function invalidateDictionaryStyles() {
  stylesLoaded = null;
  void ensureDictionaryStyles();
}

function scopeSelector(title: string): string {
  return `[data-dictionary="${title.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"]`;
}

function ensureDictionaryStyles(): Promise<void> {
  if (stylesLoaded === null) {
    stylesLoaded = dictApi
      .styles()
      .then((list) => {
        let element = document.getElementById(STYLE_ELEMENT_ID);
        if (element === null) {
          element = document.createElement("style");
          element.id = STYLE_ELEMENT_ID;
          document.head.appendChild(element);
        }
        element.textContent = list.map(({ title, styles }) => `${scopeSelector(title)} {\n${styles}\n}`).join("\n");
      })
      .catch(() => {
        stylesLoaded = null;
      });
  }
  return stylesLoaded;
}

interface Props {
  result: FindTermsResult | null;
  loading: boolean;
  error: string | null;
  emptyText?: string | undefined;
  onLookup?: ((text: string) => void) | undefined;
  compact?: boolean | undefined;
  /** 每个词条词头右边的按钮 */
  entryActions?: ((entry: TermDictionaryEntry) => ReactNode) | undefined;
}

export function LookupResults({ result, loading, error, emptyText, onLookup, compact = false, entryActions }: Props) {
  const collapsed = useCollapsedDictionaries();
  const [showAll, setShowAll] = useState(false);

  useEffect(() => {
    void ensureDictionaryStyles();
  }, []);

  // 换了一次查词，重新只显示前几条
  useEffect(() => setShowAll(false), [result]);

  const entries = result?.dictionaryEntries ?? [];
  const titles = useMemo(
    () => [...new Set(entries.flatMap((entry) => entry.definitions.map((d) => d.dictionary)))],
    [entries],
  );

  if (error !== null) {
    return <p className="dict-empty warn">{error}</p>;
  }
  if (result === null) {
    return loading ? <p className="dict-empty muted">查词中…</p> : null;
  }
  if (entries.length === 0) {
    return <p className="dict-empty muted">{loading ? "查词中…" : (emptyText ?? "没有查到")}</p>;
  }

  const allCollapsed = titles.length > 0 && titles.every((title) => collapsed.has(title));
  const limit = compact && !showAll ? COMPACT_ENTRY_LIMIT : entries.length;
  return (
    <div className="dict-results" data-loading={loading ? "true" : undefined} data-compact={compact ? "true" : undefined}>
      <div className="dict-results-toolbar">
        <span>{entries.length} 条</span>
        {titles.length > 0 && (
          <button onClick={() => setDictionariesCollapsed(titles, !allCollapsed)}>
            {allCollapsed ? "展开全部词典" : "折叠全部词典"}
          </button>
        )}
      </div>
      {entries.slice(0, limit).map((entry, i) => (
        <TermEntry
          key={`${entry.headwords[0]?.term ?? ""}-${i}`}
          entry={entry}
          onLookup={onLookup}
          compact={compact}
          actions={entryActions?.(entry)}
        />
      ))}
      {entries.length > limit && (
        <button className="dict-more" onClick={() => setShowAll(true)}>
          再显示 {entries.length - limit} 条
        </button>
      )}
    </div>
  );
}
