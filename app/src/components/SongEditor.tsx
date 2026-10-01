/**
 * 编辑曲目信息：歌名、歌手、年份、专辑、流派。只改库（和 metadata/songs.csv），不写音频文件的标签。
 */

import { useState } from "react";

import type { Track } from "../api";
import { FIELD_LABELS, libraryApi, type EditReport, type SongEdit } from "../libraryAdmin";

interface Props {
  track: Track;
  onSaved: (report: EditReport) => void;
  onCancel: () => void;
  onError: (message: string) => void;
}

const FIELDS: (keyof SongEdit)[] = ["title", "artist", "album", "year", "genre"];

export function SongEditor({ track, onSaved, onCancel, onError }: Props) {
  const original: SongEdit = {
    title: track.title,
    artist: track.artist,
    year: track.year ?? "",
    album: track.album ?? "",
    genre: track.genre ?? "",
  };
  const [draft, setDraft] = useState<SongEdit>(original);
  const [saving, setSaving] = useState(false);
  const dirty = FIELDS.some((f) => draft[f].trim() !== original[f]);
  const valid = draft.title.trim() !== "" && draft.artist.trim() !== "";

  const save = async () => {
    setSaving(true);
    try {
      onSaved(await libraryApi.edit(track.id, draft));
    } catch (err) {
      onError(String((err as { message?: string })?.message ?? err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <form
      className="song-editor"
      onSubmit={(e) => {
        e.preventDefault();
        if (dirty && valid && !saving) void save();
      }}
    >
      {FIELDS.map((field) => (
        <label key={field} className={`song-field ${field}`}>
          <span>{FIELD_LABELS[field]}</span>
          <input
            value={draft[field]}
            onChange={(e) => setDraft({ ...draft, [field]: e.target.value })}
            placeholder={field === "artist" ? "多人用 / 隔开" : ""}
            autoFocus={field === "title"}
          />
        </label>
      ))}
      <p className="muted small song-editor-note">
        只改曲库，不写音频文件的标签。改歌手会按新名字重写演唱署名（多人用 / 隔开）；改专辑会重新归到对应专辑。
      </p>
      <div className="song-editor-actions">
        <button type="button" className="ghost" onClick={onCancel} disabled={saving}>
          取消
        </button>
        <button type="submit" className="primary" disabled={!dirty || !valid || saving}>
          {saving ? "保存中…" : "保存"}
        </button>
      </div>
    </form>
  );
}
