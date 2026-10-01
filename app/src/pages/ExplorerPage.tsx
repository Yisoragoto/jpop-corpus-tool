/**
 * Corpus Explorer：按**人**这个维度看语料。
 *
 * 这是把 `songs.artist` 字符串换成 `people` + `track_credits` 实体之后
 * 才做得出来的页面——「这个作曲家写过哪些歌」「这两个人合作过几首」
 * 在字符串时代只能靠 LIKE 拼，现在是一次自连接。
 *
 * 作词 / 作曲 / 编曲 三个维度的数据来自 LRC 文件里那些一直被
 * 导入流程当噪音丢掉的信用行（206 个文件里有 99 个带着）。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import {
  api,
  ROLE_LABELS,
  type Collaborator,
  type PersonSummary,
  type Track,
} from "../api";
import { Stat } from "../components/Stat";
import { Avatar } from "../components/Avatar";
import type { LibraryActions } from "../useLibrary";

const ROLES = ["performer", "composer", "lyricist", "arranger"] as const;

interface Props {
  actions: LibraryActions;
  onNavigate: () => void;
  onError: (message: string) => void;
  /** 从命令面板跳过来时要聚焦的人。消费掉就清空。 */
  focusPersonId: number | null;
  onFocusConsumed: () => void;
}

export function ExplorerPage({
  actions,
  onNavigate,
  onError,
  focusPersonId,
  onFocusConsumed,
}: Props) {
  const [role, setRole] = useState<string>("performer");
  const [people, setPeople] = useState<PersonSummary[]>([]);
  const [selected, setSelected] = useState<PersonSummary | null>(null);
  const [works, setWorks] = useState<Track[]>([]);
  const [peers, setPeers] = useState<Collaborator[]>([]);
  const [loading, setLoading] = useState(false);
  // 聚焦流程期间抑制「换角色就清空」——那个 effect 会先于选中跑
  const focusRef = useRef<number | null>(null);
  // select 定义在下面，effect 里要用它，转一道 ref
  const selectRef = useRef<((person: PersonSummary) => Promise<void>) | null>(null);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    api
      .peopleByRole(role, 300)
      .then((rows) => {
        if (!alive) return;
        setPeople(rows);
        // 换角色时清掉右侧，否则会显示上一个角色的人。
        // 但从面板聚焦过来时 role 是跟着人变的，那次不能清。
        if (focusRef.current === null) {
          // 先选上第一个人：右边两栏空着的话，这一页看上去像没加载出来
          const first = rows[0];
          if (first === undefined) {
            setSelected(null);
            setWorks([]);
            setPeers([]);
          } else {
            void selectRef.current?.(first);
          }
        }
      })
      .catch((err) => onError(String(err.message ?? err)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [role, onError]);

  const select = useCallback(
    async (person: PersonSummary) => {
      setSelected(person);
      try {
        const [w, p] = await Promise.all([
          api.worksByPerson(person.id),
          api.collaborators(person.id, 30),
        ]);
        setWorks(w);
        setPeers(p);
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      }
    },
    [onError],
  );
  selectRef.current = select;

  // 命令面板跳过来：直接按 id 取人并选中。
  // 不去左侧列表里找——那个人可能不在当前角色的列表里
  // （比如按「作曲」列着，搜到的却是个只演唱过的人）。
  useEffect(() => {
    if (focusPersonId === null) return;
    let alive = true;
    focusRef.current = focusPersonId;
    api
      .personById(focusPersonId)
      .then((person) => {
        if (!alive || !person) return;
        // 切到这个人的主角色，左侧列表才会包含他
        if (person.role) setRole(person.role);
        void select(person);
      })
      .catch((err) => onError(String(err.message ?? err)))
      .finally(() => {
        focusRef.current = null;
        if (alive) onFocusConsumed();
      });
    return () => {
      alive = false;
    };
  }, [focusPersonId, select, onError, onFocusConsumed]);

  const openTrack = useCallback(
    async (songId: string) => {
      onNavigate();
      await actions.openTrackById(songId, { autoPlay: false });
    },
    [actions, onNavigate],
  );

  // 作品的年份跨度——一个人的活跃期
  const years = works.map((w) => w.year).filter(Boolean).sort();
  const span = years.length ? `${years[0]} — ${years[years.length - 1]}` : "";

  return (
    <div className="columns explorer">
      <aside className="pane library">
        <div className="segmented role-picker">
          {ROLES.map((r) => (
            <button key={r} className={role === r ? "on" : ""} onClick={() => setRole(r)}>
              {ROLE_LABELS[r] ?? r}
            </button>
          ))}
        </div>
        <div className="list">
          {loading && <p className="muted pad">加载中…</p>}
          {!loading && people.length === 0 && (
            <p className="muted pad">这个角色下还没有数据</p>
          )}
          {people.map((person) => (
            <button
              key={person.id}
              className={`track with-cover ${selected?.id === person.id ? "active" : ""}`}
              onClick={() => void select(person)}
            >
              <Avatar path={person.imagePath} name={person.name} size={36} />
              <span className="track-lines">
                <span className="track-title">{person.name}</span>
                <span className="track-meta">{person.trackCount} 首</span>
              </span>
            </button>
          ))}
        </div>
      </aside>

      <main className="pane person">
        {!selected && <p className="muted pad">从左侧选一个人</p>}
        {selected && (
          <>
            <div className="track-head track-head-row">
              <Avatar path={selected.imagePath} name={selected.name} size={96} />
              <div className="track-head-text">
                <h1>{selected.name}</h1>
                <p className="muted">
                  {ROLE_LABELS[selected.role] ?? selected.role}
                  {span && ` · ${span}`}
                </p>
                <div className="numbers">
                  <Stat label="作品" value={works.length} align="start" />
                  <Stat label="合作者" value={peers.length} align="start" />
                  <Stat
                    label="专辑"
                    value={new Set(works.map((w) => w.albumId).filter(Boolean)).size}
                    align="start"
                  />
                </div>
              </div>
            </div>

            <h3>作品</h3>
            <div className="work-list">
              {works.map((track) => (
                <button
                  key={`${track.id}-${track.title}`}
                  className="work"
                  onClick={() => void openTrack(track.id)}
                >
                  <span className="work-year">{track.year || "—"}</span>
                  <span className="work-title">{track.title}</span>
                  <span className="work-meta">{track.album}</span>
                </button>
              ))}
              {works.length === 0 && <p className="muted">没有作品记录</p>}
            </div>
          </>
        )}
      </main>

      <aside className="pane corpus">
        {selected && (
          <>
            <h3>合作者</h3>
            <p className="muted small">同一首歌里一起出现过的人</p>
            <div className="examples">
              {peers.map((peer) => (
                <button
                  key={peer.personId}
                  className="example with-avatar"
                  onClick={() =>
                    void select({
                      id: peer.personId,
                      name: peer.name,
                      role,
                      trackCount: peer.sharedTracks,
                      imagePath: peer.imagePath,
                    })
                  }
                >
                  <Avatar path={peer.imagePath} name={peer.name} size={28} />
                  <span className="ex-lines">
                    <span className="ex-text">{peer.name}</span>
                    <span className="ex-meta">
                      {peer.sharedTracks} 首共同 ·{" "}
                      {peer.roles.map((r) => ROLE_LABELS[r] ?? r).join(" / ")}
                    </span>
                  </span>
                </button>
              ))}
              {peers.length === 0 && (
                <p className="muted">
                  没有合作者。多数歌只有一位歌手，图谱的丰富度取决于数据本身。
                </p>
              )}
            </div>
          </>
        )}
      </aside>
    </div>
  );
}
