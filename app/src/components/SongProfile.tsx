/**
 * 曲库右栏「这首歌」下面的语料画像：从一首歌进入语料分析。
 *
 * - 这首歌有多少行是重复的（副歌）——直接影响它在词频里的分量；
 * - 相对其余歌的特征词（对数似然，重复行只算一次），点一个去检索页看它在别的歌里怎么用。
 *
 * 计算在 Rust（`analysis::song_profile`），macOS 客户端的歌词面板用的是同一个。
 */

import { useEffect, useState } from "react";

import { api, POS_LABELS, type SongProfile as Profile } from "../api";

export function SongProfile({ songId, onSearchWord }: { songId: string; onSearchWord: (lemma: string) => void }) {
  const [profile, setProfile] = useState<Profile | null>(null);

  useEffect(() => {
    let cancelled = false;
    setProfile(null);
    api
      .songProfile(songId, 8)
      .then((p) => !cancelled && setProfile(p))
      // 没分词的歌、空库：画像出不来就不显示，不打断看歌
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [songId]);

  if (!profile || profile.tokens === 0) return null;
  const share = profile.lines > 0 ? profile.repeatedLines / profile.lines : 0;

  return (
    <div className="song-profile">
      <h3 className="side-head">语料画像</h3>
      <p className="small">
        {profile.lines} 行里 <b>{profile.repeatedLines}</b> 行是重复（{Math.round(share * 100)}%）；
        {profile.tokens} 词，去掉重复行后 {profile.distinctTokens} 词。
      </p>
      {profile.keywords.length > 0 && (
        <>
          <p className="muted small">比其余歌明显多用的词（点了去检索）：</p>
          <div className="chips">
            {profile.keywords.map((k) => (
              <button
                key={`${k.lemma}:${k.pos}`}
                className="chip link"
                onClick={() => onSearchWord(k.lemma)}
                title={`${POS_LABELS[k.pos] ?? k.pos} · 这首 ${k.targetFreq} 次 / 其余 ${k.referenceFreq} 次 · LL ${k.logLikelihood.toFixed(1)}`}
              >
                {k.lemma}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
