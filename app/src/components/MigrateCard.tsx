/**
 * 设置 → 曲库维护 → 从别的语料库目录迁移数据。
 *
 * 0.1.x 的库在项目目录里，新装的程序默认用 `%LOCALAPPDATA%\JPOP Corpus Tool`，
 * 「切换目录」只能二选一，这里要的是**合并**：旧库的歌、歌词、分词、署名、
 * 收听记录、词典都搬过来，当前库里已经有的那几首不动。
 *
 * 先预览（只读）再确认：会搬多少、跳过多少，心里有数了再写。
 */

import { useCallback, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "../events";

import { api, type MigrateOutcome, type MigratePlan } from "../api";
import { backfillNote } from "../backfill";
import { retryFurigana } from "../lyricsDisplay";
import { CommandButton } from "./CommandButton";
import { SettingCard, SettingNote } from "./SettingCard";
import { tr } from "../i18n";

interface Props {
  icon: React.ReactNode;
  onError: (message: string) => void;
  /** 库变了，外壳重新拉总览 */
  onChanged: () => void;
}

export function MigrateCard({ icon, onError, onChanged }: Props) {
  const [open, setOpen] = useState(false);
  const [source, setSource] = useState("");
  const [plan, setPlan] = useState<MigratePlan | null>(null);
  const [withDicts, setWithDicts] = useState(true);
  const [withHistory, setWithHistory] = useState(true);
  const [busy, setBusy] = useState<"" | "planning" | "running">("");
  const [step, setStep] = useState("");
  const [outcome, setOutcome] = useState<MigrateOutcome | null>(null);

  const pick = useCallback(async () => {
    try {
      const picked = await openDialog({ directory: true, title: tr("选择要迁移过来的语料库目录（含 corpus.db）") });
      if (typeof picked !== "string") return;
      setSource(picked);
      setOutcome(null);
      setBusy("planning");
      setPlan(await api.migratePlan(picked));
    } catch (err) {
      setPlan(null);
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy("");
    }
  }, [onError]);

  const run = useCallback(async () => {
    if (source === "") return;
    setBusy("running");
    setStep(tr("准备中…"));
    const off = await listen<string>("migrate://progress", (event) => setStep(event.payload));
    try {
      const result = await api.migrateRun(source, { dictionaries: withDicts, history: withHistory });
      setOutcome(result);
      setPlan(null);
      if (result.tokenized.songs > 0) retryFurigana();
      onChanged();
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      off();
      setBusy("");
      setStep("");
    }
  }, [source, withDicts, withHistory, onChanged, onError]);

  return (
    <>
      <SettingCard
        icon={icon}
        title={tr("从别的语料库目录迁移数据")}
        description={tr("把另一个目录里的歌、歌词、分词、署名、收听记录、词典和分词词典搬过来；当前库里已经有的那几首会跳过。音频文件不复制，原路径照旧可用。")}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="setting-chevron" aria-hidden="true">
          {open ? "⌄" : "›"}
        </span>
      </SettingCard>

      {open && (
        <div className="migrate-panel">
          <div className="migrate-row">
            <CommandButton
              icon="folder"
              label={busy === "planning" ? tr("读取中…") : tr("选目录…")}
              onClick={() => void pick()}
              disabled={busy !== ""}
            />
            <span className="muted small">{source || tr("还没选")}</span>
          </div>

          {plan !== null && (
            <>
              <div className="migrate-stats">
                <b>{plan.newSongs}</b>{" "}{tr("首要搬过来")}{plan.duplicateSongs > 0 && <span className="muted">{tr("（{duplicateSongs} 首当前库已经有了，跳过）", { duplicateSongs: plan.duplicateSongs })}</span>}
              </div>
              <ul className="list compact">
                <li>{tr("歌词 {lyricLines} 行 · 分词 {tokens}", { lyricLines: plan.lyricLines.toLocaleString(), tokens: plan.tokens.toLocaleString() })}</li>
                <li>{tr("人 {people} · 专辑 {albums} · 署名 {credits}", { people: plan.people, albums: plan.albums, credits: plan.credits })}</li>
                <li>{tr("收听记录 {plays} · 收藏 {favorites} · 分词校正 {corrections}", { plays: plan.plays, favorites: plan.favorites, corrections: plan.corrections })}</li>
                {plan.dictTerms > 0 && (
                  <li>{tr("词典词条 {dictTerms} · JLPT 缓存 {jlptRows}", { dictTerms: plan.dictTerms.toLocaleString(), jlptRows: plan.jlptRows.toLocaleString() })}{plan.targetHasDictionaries && <span className="warn">{" "}{tr("（当前库已经有词典表，不会覆盖）")}</span>}
                  </li>
                )}
              </ul>
              {plan.samples.length > 0 && (
                <p className="muted small">{tr("例如：{slice}…", { slice: plan.samples.slice(0, 5).join(tr("、")) })}</p>
              )}

              <div className="migrate-row">
                <label className="check">
                  <input type="checkbox" checked={withDicts} onChange={(e) => setWithDicts(e.target.checked)} />{tr("一起搬词典（制卡的释义和统计页的 JLPT 等级靠它）")}</label>
              </div>
              <div className="migrate-row">
                <label className="check">
                  <input type="checkbox" checked={withHistory} onChange={(e) => setWithHistory(e.target.checked)} />{tr("一起搬收听记录和收藏")}</label>
              </div>

              <div className="migrate-row">
                <CommandButton
                  icon="import"
                  label={busy === "running" ? tr("迁移中…") : tr("开始迁移 {newSongs} 首", { newSongs: plan.newSongs })}
                  onClick={() => void run()}
                  disabled={busy !== "" || plan.newSongs + plan.dictTerms === 0}
                  primary
                  title={tr("写库。中途出错会整个回滚，什么都不会改")}
                />
                {busy === "running" && <span className="muted small">{step}</span>}
              </div>
            </>
          )}

          {outcome !== null && (
            <SettingNote>{tr("搬完了：歌 {songs} 首（跳过 {skipped}）、歌词 {lyricLines} 行、 分词 {tokens}、署名 {credits}、收听 {plays}； 封面 {coversCopied}、歌词文件 {lyricsCopied}、歌手照片 {artistPhotosCopied}", { songs: outcome.songs, skipped: outcome.skipped, lyricLines: outcome.lyricLines.toLocaleString(), tokens: outcome.tokens.toLocaleString(), credits: outcome.credits, plays: outcome.plays, coversCopied: outcome.coversCopied, lyricsCopied: outcome.lyricsCopied, artistPhotosCopied: outcome.artistPhotosCopied })}{outcome.dictTerms > 0 && tr("；词典词条 {dictTerms}", { dictTerms: outcome.dictTerms.toLocaleString() })}
              {outcome.dictionariesCopied &&
                tr("、词典库 {toFixed} MB", { toFixed: (outcome.dictionariesBytes / 1024 / 1024).toFixed(0) })}
              {outcome.sudachiCopied &&
                tr("、分词词典 {toFixed} MB（振假名已可用）", { toFixed: (outcome.sudachiBytes / 1024 / 1024).toFixed(0) })}
              {tr("。")}{backfillNote(outcome.tokenized, "")}
              {outcome.warnings.length > 0 && <>{" "}{tr("另有 {length} 条提醒：{warnings}", { length: outcome.warnings.length, warnings: outcome.warnings.join(tr("；")) })}</>}
            </SettingNote>
          )}
        </div>
      )}
    </>
  );
}
