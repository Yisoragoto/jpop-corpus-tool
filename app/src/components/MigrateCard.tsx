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
import { listen } from "@tauri-apps/api/event";

import { api, type MigrateOutcome, type MigratePlan } from "../api";
import { CommandButton } from "./CommandButton";
import { SettingCard, SettingNote } from "./SettingCard";

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
      const picked = await openDialog({ directory: true, title: "选择要迁移过来的语料库目录（含 corpus.db）" });
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
    setStep("准备中…");
    const off = await listen<string>("migrate://progress", (event) => setStep(event.payload));
    try {
      const result = await api.migrateRun(source, { dictionaries: withDicts, history: withHistory });
      setOutcome(result);
      setPlan(null);
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
        title="从别的语料库目录迁移数据"
        description="把另一个目录里的歌、歌词、分词、署名、收听记录、词典和分词词典搬过来；当前库里已经有的那几首会跳过。音频文件不复制，原路径照旧可用。"
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
              label={busy === "planning" ? "读取中…" : "选目录…"}
              onClick={() => void pick()}
              disabled={busy !== ""}
            />
            <span className="muted small">{source || "还没选"}</span>
          </div>

          {plan !== null && (
            <>
              <div className="migrate-stats">
                <b>{plan.newSongs}</b> 首要搬过来
                {plan.duplicateSongs > 0 && <span className="muted">（{plan.duplicateSongs} 首当前库已经有了，跳过）</span>}
              </div>
              <ul className="list compact">
                <li>歌词 {plan.lyricLines.toLocaleString()} 行 · 分词 {plan.tokens.toLocaleString()}</li>
                <li>
                  人 {plan.people} · 专辑 {plan.albums} · 署名 {plan.credits}
                </li>
                <li>
                  收听记录 {plan.plays} · 收藏 {plan.favorites} · 分词校正 {plan.corrections}
                </li>
                {plan.dictTerms > 0 && (
                  <li>
                    词典词条 {plan.dictTerms.toLocaleString()} · JLPT 缓存 {plan.jlptRows.toLocaleString()}
                    {plan.targetHasDictionaries && <span className="warn"> （当前库已经有词典表，不会覆盖）</span>}
                  </li>
                )}
              </ul>
              {plan.samples.length > 0 && (
                <p className="muted small">例如：{plan.samples.slice(0, 5).join("、")}…</p>
              )}

              <div className="migrate-row">
                <label className="check">
                  <input type="checkbox" checked={withDicts} onChange={(e) => setWithDicts(e.target.checked)} />
                  一起搬词典（制卡的释义和统计页的 JLPT 等级靠它）
                </label>
              </div>
              <div className="migrate-row">
                <label className="check">
                  <input type="checkbox" checked={withHistory} onChange={(e) => setWithHistory(e.target.checked)} />
                  一起搬收听记录和收藏
                </label>
              </div>

              <div className="migrate-row">
                <CommandButton
                  icon="import"
                  label={busy === "running" ? "迁移中…" : `开始迁移 ${plan.newSongs} 首`}
                  onClick={() => void run()}
                  disabled={busy !== "" || plan.newSongs + plan.dictTerms === 0}
                  primary
                  title="写库。中途出错会整个回滚，什么都不会改"
                />
                {busy === "running" && <span className="muted small">{step}</span>}
              </div>
            </>
          )}

          {outcome !== null && (
            <SettingNote>
              搬完了：歌 {outcome.songs} 首（跳过 {outcome.skipped}）、歌词 {outcome.lyricLines.toLocaleString()} 行、
              分词 {outcome.tokens.toLocaleString()}、署名 {outcome.credits}、收听 {outcome.plays}；
              封面 {outcome.coversCopied}、歌词文件 {outcome.lyricsCopied}、歌手照片 {outcome.artistPhotosCopied}
              {outcome.dictTerms > 0 && `；词典词条 ${outcome.dictTerms.toLocaleString()}`}
              {outcome.dictionariesCopied &&
                `、词典库 ${(outcome.dictionariesBytes / 1024 / 1024).toFixed(0)} MB`}
              {outcome.sudachiCopied &&
                `、分词词典 ${(outcome.sudachiBytes / 1024 / 1024).toFixed(0)} MB（振假名已可用）`}
              。
              {outcome.warnings.length > 0 && <> 另有 {outcome.warnings.length} 条提醒：{outcome.warnings.join("；")}</>}
            </SettingNote>
          )}
        </div>
      )}
    </>
  );
}
