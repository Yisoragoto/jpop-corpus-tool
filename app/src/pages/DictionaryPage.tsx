import { Icon } from "../components/Icon";
/**
 * 词典页：只做一件事——查词。
 *
 * 管理（导入、启用、排序、删除）搬去了设置页：查词的时候没人在管词典，
 * 管词典的时候也不是在查词，两件事挤在一屏里只会互相打扰。
 * **一本词典都没有时**，这一页给的是导入入口而不是一个空输入框。
 *
 * 词典存在独立的 dictionaries.db，和语料库分开。
 *
 * 查到一个词，右边同时打开「词的去处」：这个词在语料里出现在哪些歌，点一首就从这个词开始放。
 * 释义和真实用例并排，查词不再是死胡同。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "../events";

import { CommandButton } from "../components/CommandButton";
import { LoadError } from "../components/LoadError";
import {
  dictApi,
  type DictImportDone,
  type DictionaryInfo,
  type FindTermsResult,
  type LegacySource,
} from "../dict/api";
import { errorMessage, pickDictionaryFiles } from "../dict/DictionaryManager";
import { invalidateDictionaryStyles, LookupResults } from "../dict/LookupResults";
import { WithWordPanel, wordOf, type WordPanelLinks } from "../components/WordWhere";
import { tr } from "../i18n";

interface Props {
  onError: (message: string) => void;
  /** 去设置页管理词典 */
  onOpenSettings: () => void;
  /** 右边「词的去处」要接的线（App 给） */
  wordLinks: WordPanelLinks;
}

export function DictionaryPage({ onError, onOpenSettings, wordLinks }: Props) {
  const [dictionaries, setDictionaries] = useState<DictionaryInfo[] | null>(null);
  const [legacy, setLegacy] = useState<LegacySource[]>([]);
  const [importing, setImporting] = useState(false);
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<FindTermsResult | null>(null);
  const [lookupError, setLookupError] = useState<string | null>(null);
  const [looking, setLooking] = useState(false);
  const errorRef = useRef(onError);
  errorRef.current = onError;
  /** 词典列表读不出来：页面上说，并且能重试 */
  const [loadError, setLoadError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const [list, sources] = await Promise.all([dictApi.list(), dictApi.legacySources()]);
      setDictionaries(list);
      setLegacy(sources);
      setLoadError(null);
    } catch (e) {
      setLoadError(errorMessage(e));
      errorRef.current(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void reload();
    void dictApi.isImporting().then(setImporting).catch(() => undefined);
  }, [reload]);

  // 在设置页导完词典，这一页要跟着变（空状态换成查词框）
  useEffect(() => {
    const offDone = listen<DictImportDone>("dict://done", () => {
      setImporting(false);
      invalidateDictionaryStyles();
      void reload();
    });
    const offProgress = listen("dict://progress", () => setImporting(true));
    return () => {
      void offDone.then((f) => f());
      void offProgress.then((f) => f());
    };
  }, [reload]);

  // 输入停下 200ms 再查；词典列表变了也重查一次
  useEffect(() => {
    const text = query.trim();
    if (text === "") {
      setResult(null);
      setLookupError(null);
      return;
    }
    let alive = true;
    const timer = window.setTimeout(() => {
      setLooking(true);
      dictApi
        .lookup(text)
        .then(
          (r) => {
            if (!alive) return;
            setResult(r);
            setLookupError(null);
          },
          (e: unknown) => {
            if (alive) setLookupError(errorMessage(e));
          },
        )
        .finally(() => {
          if (alive) setLooking(false);
        });
    }, 200);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [query, dictionaries]);

  // 查到的第一个词头：右边列出它在语料里的出处。关掉之后换了词才再打开
  const head = result?.dictionaryEntries[0]?.headwords[0]?.term ?? null;
  const [closedFor, setClosedFor] = useState<string | null>(null);
  const panelWord = useMemo(() => (head !== null && head !== closedFor ? wordOf(head) : null), [head, closedFor]);

  if (dictionaries === null && loadError !== null) return <LoadError message={loadError} onRetry={() => void reload()} />;
  if (dictionaries === null) return <div className="page muted pad" role="status">{tr("加载中…")}</div>;

  // ── 一本都没有：给入口，不给空输入框 ──
  if (dictionaries.length === 0) {
    const migratable = legacy.filter((s) => s.exists && !s.imported);
    return (
      <div className="page dict-page empty">
        <div className="empty-state">
          <Icon name="dict" size={46} />
          <h2>{tr("还没有词典")}</h2>
          <p>
            {tr("导入 Yomitan 格式的 zip 就能查词：Jitendex、大辞泉、明鏡、JPDB 词频、アクセント辞典都认。导进来之后，曲库里点歌词上的词也会出释义。")}
          </p>
          <div className="empty-actions">
            <CommandButton
              icon="import"
              label={importing ? tr("正在导入…") : tr("导入 .zip")}
              onClick={() => {
                void pickDictionaryFiles(onError).then(async (paths) => {
                  if (paths.length === 0) return;
                  await dictApi.importStart(paths);
                  setImporting(true);
                });
              }}
              disabled={importing}
              primary
            />
            {migratable.length > 0 && (
              <CommandButton
                icon="folder"
                label={tr("从旧版迁移 {length} 本", { length: migratable.length })}
                onClick={() => void dictApi.importStart(migratable.map((s) => s.path)).then(() => setImporting(true))}
                disabled={importing}
                title={migratable.map((s) => s.name).join("\n")}
              />
            )}
          </div>
          <button className="link-btn" onClick={onOpenSettings}>{tr("在设置里管理词典")}</button>
        </div>
      </div>
    );
  }

  // ── 正常：整页就是查词 ──
  const enabled = dictionaries.filter((d) => d.enabled).length;
  return (
    <WithWordPanel word={panelWord} onClose={() => setClosedFor(head)} links={wordLinks}>
    <div className="page dict-page">
      <div className="dict-search">
        <input
          className="search dict-query"
          lang="ja"
          placeholder={tr("查词：输入日语，活用形、片假名、罗马字都可以")}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          autoFocus
        />
        <div className="dict-subbar">
          <span className="muted small">{tr("{enabled} / {length} 本启用", { enabled, length: dictionaries.length })}</span>
          <button className="link-btn" onClick={onOpenSettings}>{tr("管理词典")}</button>
        </div>
        <LookupResults result={result} loading={looking} error={lookupError} onLookup={setQuery} />
      </div>
    </div>
    </WithWordPanel>
  );
}
