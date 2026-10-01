/**
 * 词典页：只做一件事——查词。
 *
 * 管理（导入、启用、排序、删除）搬去了设置页：查词的时候没人在管词典，
 * 管词典的时候也不是在查词，两件事挤在一屏里只会互相打扰。
 * **一本词典都没有时**，这一页给的是导入入口而不是一个空输入框。
 *
 * 词典存在独立的 dictionaries.db，和语料库分开。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { CommandButton } from "../components/CommandButton";
import {
  dictApi,
  type DictImportDone,
  type DictionaryInfo,
  type FindTermsResult,
  type LegacySource,
} from "../dict/api";
import { errorMessage, pickDictionaryFiles } from "../dict/DictionaryManager";
import { invalidateDictionaryStyles, LookupResults } from "../dict/LookupResults";

interface Props {
  onError: (message: string) => void;
  /** 去设置页管理词典 */
  onOpenSettings: () => void;
}

export function DictionaryPage({ onError, onOpenSettings }: Props) {
  const [dictionaries, setDictionaries] = useState<DictionaryInfo[] | null>(null);
  const [legacy, setLegacy] = useState<LegacySource[]>([]);
  const [importing, setImporting] = useState(false);
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<FindTermsResult | null>(null);
  const [lookupError, setLookupError] = useState<string | null>(null);
  const [looking, setLooking] = useState(false);
  const errorRef = useRef(onError);
  errorRef.current = onError;

  const reload = useCallback(async () => {
    try {
      const [list, sources] = await Promise.all([dictApi.list(), dictApi.legacySources()]);
      setDictionaries(list);
      setLegacy(sources);
    } catch (e) {
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

  if (dictionaries === null) return <div className="page muted pad">加载中…</div>;

  // ── 一本都没有：给入口，不给空输入框 ──
  if (dictionaries.length === 0) {
    const migratable = legacy.filter((s) => s.exists && !s.imported);
    return (
      <div className="page dict-page empty">
        <div className="empty-state">
          <svg viewBox="0 0 24 24" width="46" height="46" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M5 5.5A1.5 1.5 0 0 1 6.5 4H19v16H6.5A1.5 1.5 0 0 1 5 18.5z" />
            <path d="M9 4v16" />
          </svg>
          <h2>还没有词典</h2>
          <p>
            导入 Yomitan 格式的 zip 就能查词：Jitendex、大辞泉、明鏡、JPDB 词频、アクセント辞典都认。
            导进来之后，曲库里点歌词上的词也会出释义。
          </p>
          <div className="empty-actions">
            <CommandButton
              icon="import"
              label={importing ? "正在导入…" : "导入 .zip"}
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
                label={`从旧版迁移 ${migratable.length} 本`}
                onClick={() => void dictApi.importStart(migratable.map((s) => s.path)).then(() => setImporting(true))}
                disabled={importing}
                title={migratable.map((s) => s.name).join("\n")}
              />
            )}
          </div>
          <button className="link-btn" onClick={onOpenSettings}>
            在设置里管理词典
          </button>
        </div>
      </div>
    );
  }

  // ── 正常：整页就是查词 ──
  const enabled = dictionaries.filter((d) => d.enabled).length;
  return (
    <div className="page dict-page">
      <div className="dict-search">
        <input
          className="search dict-query"
          lang="ja"
          placeholder="查词：输入日语，活用形、片假名、罗马字都可以"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          autoFocus
        />
        <div className="dict-subbar">
          <span className="muted small">
            {enabled} / {dictionaries.length} 本启用
          </span>
          <button className="link-btn" onClick={onOpenSettings}>
            管理词典
          </button>
        </div>
        <LookupResults result={result} loading={looking} error={lookupError} onLookup={setQuery} />
      </div>
    </div>
  );
}
