/**
 * 设置 → 曲库维护 → 补齐缺失分词。
 *
 * 没有分词词典的时候导入 / 补歌词，歌词照常入库，分词那一步被跳过了。
 * 表现很迷惑：**那几首歌点词查不了、也不显示振假名**，而别的歌都正常——
 * 因为界面上这两件事都是按 token 渲染的（`line.tokens.length === 0` 时只画纯文本）。
 *
 * 词典补上之后这些歌不会自己好，要重新分一次。这张卡就做这件事：
 * 先告诉你有几首、是哪几首，补完一个事务提交。你校正过的分词会按原文套回去。
 */

import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { api, type Tokenized, type UntokenizedSong } from "../api";
import { retryFurigana } from "../lyricsDisplay";
import { CommandButton } from "./CommandButton";
import { SettingCard, SettingNote } from "./SettingCard";

interface Props {
  icon: React.ReactNode;
  onError: (message: string) => void;
  /** 库变了，外壳重新拉总览 */
  onChanged: () => void;
}

export function TokenizeCard({ icon, onError, onChanged }: Props) {
  const [songs, setSongs] = useState<UntokenizedSong[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [step, setStep] = useState("");
  const [done, setDone] = useState<Tokenized | null>(null);

  const refresh = useCallback(() => {
    void api
      .tokenizeMissingList()
      .then(setSongs)
      .catch(() => setSongs([]));
  }, []);

  useEffect(refresh, [refresh]);

  const run = useCallback(async () => {
    setBusy(true);
    setStep("准备中…");
    const off = await listen<string>("tokenize://progress", (event) => setStep(event.payload));
    try {
      const report = await api.tokenizeMissingRun();
      setDone(report);
      retryFurigana();
      refresh();
      onChanged();
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      off();
      setBusy(false);
      setStep("");
    }
  }, [onChanged, onError, refresh]);

  // 没有缺的就不占地方
  if (songs !== null && songs.length === 0 && done === null) return null;

  const count = songs?.length ?? 0;

  return (
    <>
      <SettingCard
        icon={icon}
        title="补齐缺失分词"
        description={
          count > 0
            ? `${count} 首歌有歌词但没有分词——那几首点词查不了、也不显示振假名。多半是在还没有分词词典的时候入库的。`
            : "都有分词了。"
        }
      >
        {busy && <span className="muted small">{step}</span>}
        {count > 0 && (
          <CommandButton
            icon="refresh"
            label={busy ? "分词中…" : `补齐 ${count} 首`}
            onClick={() => void run()}
            disabled={busy}
            primary
            title="拿库里的歌词重新分一次词。你校正过的分词会按原文套回去"
          />
        )}
      </SettingCard>
      {songs !== null && songs.length > 0 && (
        <SettingNote>
          {songs
            .slice(0, 6)
            .map((song) => `${song.artist} · ${song.title}`)
            .join("、")}
          {songs.length > 6 && ` 等 ${songs.length} 首`}
        </SettingNote>
      )}
      {done !== null && (
        <SettingNote>
          补完了：{done.songs} 首、{done.lines} 行、{done.tokens.toLocaleString()} 个词
          {done.correctionsRestored > 0 && `，套回分词校正 ${done.correctionsRestored} 行`}。
        </SettingNote>
      )}
    </>
  );
}
