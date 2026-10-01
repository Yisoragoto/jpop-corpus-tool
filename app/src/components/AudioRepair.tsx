/**
 * 找回丢了的音频文件。对应 Python 版的「音声修復」：扫一个文件夹自动配对，或者逐首手动选文件，确认后才写库。
 *
 * 自动配对**只给有把握的**：标签（或文件名）里的歌手和歌名都对得上，或者文件名就是这首歌的编号、放在歌手同名的目录里；
 * 一首对上多个文件、一个文件对上多首都不配。Python 版是「文件名里含歌名就算」，「夜」「春」这种短歌名会配错。
 */

import { useMemo, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { AUDIO_EXTENSIONS, csvProblem, libraryApi, type MissingAudio } from "../libraryAdmin";

interface Props {
  missing: MissingAudio[];
  /** 写库成功的曲目 */
  onApplied: (songIds: string[]) => void;
  onClose: () => void;
  onError: (message: string) => void;
}

interface Pick {
  path: string;
  reason: string;
}

export function AudioRepair({ missing, onApplied, onClose, onError }: Props) {
  const [picks, setPicks] = useState<Map<string, Pick>>(new Map());
  const [errors, setErrors] = useState<Map<string, string>>(new Map());
  const [busy, setBusy] = useState<"" | "scanning" | "applying">("");
  const [scanNote, setScanNote] = useState("");

  const setPick = (songId: string, pick: Pick | null) =>
    setPicks((prev) => {
      const next = new Map(prev);
      if (pick === null) next.delete(songId);
      else next.set(songId, pick);
      return next;
    });

  const scanFolder = async () => {
    const folder = await openDialog({ directory: true, title: "选一个放着音频文件的文件夹" });
    if (typeof folder !== "string") return;
    setBusy("scanning");
    setScanNote("");
    try {
      const suggestions = await libraryApi.suggestRelinks(folder);
      setPicks((prev) => {
        const next = new Map(prev);
        // 手动选过的不覆盖
        for (const s of suggestions) if (!next.has(s.songId)) next.set(s.songId, { path: s.path, reason: s.reason });
        return next;
      });
      const open = missing.length - suggestions.length;
      setScanNote(
        suggestions.length === 0
          ? `在「${folder}」里没有找到有把握配上的文件`
          : `配上 ${suggestions.length} 首${open > 0 ? `，${open} 首没有有把握的匹配，可以逐首手动选` : ""}`,
      );
    } catch (err) {
      onError(String((err as { message?: string })?.message ?? err));
    } finally {
      setBusy("");
    }
  };

  const chooseFile = async (song: MissingAudio) => {
    const file = await openDialog({
      title: `${song.artist}《${song.title}》的音频文件`,
      filters: [{ name: "音频文件", extensions: AUDIO_EXTENSIONS }],
    });
    if (typeof file === "string") setPick(song.songId, { path: file, reason: "手动选的" });
  };

  const apply = async () => {
    setBusy("applying");
    try {
      const batch = await libraryApi.relink([...picks].map(([songId, pick]) => ({ songId, path: pick.path })));
      const failed = new Map(batch.results.filter((r) => r.error !== "").map((r) => [r.songId, r.error]));
      const done = batch.results.filter((r) => r.error === "").map((r) => r.songId);
      setErrors(failed);
      setPicks((prev) => new Map([...prev].filter(([id]) => failed.has(id))));
      const problem = csvProblem(batch.csv);
      if (problem) onError(problem);
      if (done.length > 0) onApplied(done);
    } catch (err) {
      onError(String((err as { message?: string })?.message ?? err));
    } finally {
      setBusy("");
    }
  };

  const pending = useMemo(() => missing.filter((m) => picks.has(m.songId)).length, [missing, picks]);

  return (
    <section className="audio-repair">
      <div className="audio-repair-head">
        <h2>找回音频文件</h2>
        <button className="stage-close" onClick={onClose} title="关闭" aria-label="关闭">
          ✕
        </button>
      </div>
      <p className="muted small">
        {missing.length === 0
          ? "所有曲目的音频文件都在。"
          : `${missing.length} 首歌在库里记的路径上找不到音频文件（被移动、改名或删掉了）。扫一个文件夹自动配对，或者逐首手动选；确认后才会改库。`}
      </p>
      {missing.length > 0 && (
        <>
          <div className="toolbar">
            <button className="primary" onClick={() => void scanFolder()} disabled={busy !== ""}>
              {busy === "scanning" ? "扫描中…" : "扫描文件夹自动配对…"}
            </button>
            {scanNote && <span className="muted small">{scanNote}</span>}
          </div>
          <div className="repair-list">
            {missing.map((song) => {
              const pick = picks.get(song.songId);
              const error = errors.get(song.songId);
              return (
                <div key={song.songId} className={`repair-row ${pick ? "picked" : ""}`}>
                  <div className="repair-song">
                    <span className="repair-title">{song.title}</span>
                    <span className="muted small">
                      {song.artist} · {song.songId}
                    </span>
                    <span className="repair-old small" title={song.audioPath}>
                      原路径：{song.audioPath || "（空）"}
                    </span>
                  </div>
                  <div className="repair-pick">
                    {pick ? (
                      <>
                        <span className="repair-new small" title={pick.path}>
                          → {pick.path}
                        </span>
                        <span className="muted small">{pick.reason}</span>
                      </>
                    ) : (
                      <span className="muted small">还没配上</span>
                    )}
                    {error && <span className="warn small">{error}</span>}
                  </div>
                  <div className="repair-actions">
                    <button className="link-btn" onClick={() => void chooseFile(song)} disabled={busy !== ""}>
                      选择文件…
                    </button>
                    {pick && (
                      <button className="link-btn" onClick={() => setPick(song.songId, null)} disabled={busy !== ""}>
                        不改
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
          <div className="song-editor-actions">
            <button className="primary" onClick={() => void apply()} disabled={pending === 0 || busy !== ""}>
              {busy === "applying" ? "写入中…" : `确认更新 ${pending} 首`}
            </button>
          </div>
        </>
      )}
    </section>
  );
}
