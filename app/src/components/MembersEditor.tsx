/**
 * 乐队档案上的「成员」：成员卡片 + 推测出来、等你确认的成员 + 手动编辑。
 *
 * 规则在后端（`jp_corpus::members`）：这里改的都记成「手动」，以后刮削不会覆盖；
 * 点「不是」会留一条否掉的记录，下次刮削、推测都不会再把他加回来。
 */

import { useCallback, useEffect, useState } from "react";

import { api, type Membership, type MembershipPutOutcome, type MembershipSuggestion } from "../api";
import { messageOf } from "../errors";
import { spanOf, validDate } from "../libraryIndex";
import { Avatar } from "./Avatar";
import type { DirectoryData } from "./LibraryBrowser";

const OUTCOME_NOTES: Record<MembershipPutOutcome, string> = {
  inserted: "加上了",
  updated: "改好了",
  keptExisting: "已有更可靠的记录，没改",
  rejected: "这一条之前被否掉了",
};

interface Props {
  bandId: number;
  members: Membership[];
  data: DirectoryData | null;
  onOpen: (id: number) => void;
  /** 成员变了：目录要重拉 */
  onChanged: () => void;
  onError: (message: string) => void;
}

export function MembersEditor({ bandId, members, data, onOpen, onChanged, onError }: Props) {
  const [editing, setEditing] = useState(false);
  const [suggestions, setSuggestions] = useState<MembershipSuggestion[]>([]);
  const [note, setNote] = useState("");

  useEffect(() => {
    let alive = true;
    api
      .membershipSuggestions()
      .then((all) => alive && setSuggestions(all.filter((s) => s.bandId === bandId)))
      .catch(() => alive && setSuggestions([]));
    return () => {
      alive = false;
    };
  }, [bandId, members]);

  const run = useCallback(
    async (job: () => Promise<MembershipPutOutcome | void>) => {
      try {
        const outcome = await job();
        setNote(outcome ? OUTCOME_NOTES[outcome] : "");
        onChanged();
      } catch (err) {
        onError(`改成员没成功：${messageOf(err)}`);
      }
    },
    [onChanged, onError],
  );

  if (members.length === 0 && suggestions.length === 0 && !editing) {
    return (
      <section className="member-cards empty">
        <button className="section-toggle" onClick={() => setEditing(true)}>
          添加成员
        </button>
      </section>
    );
  }

  return (
    <section className="member-cards">
      <h2 className="section-label section-head">
        成员
        <button className="section-toggle" onClick={() => setEditing((v) => !v)}>
          {editing ? "完成" : "编辑"}
        </button>
        {note && <span className="section-count">{note}</span>}
      </h2>

      {!editing && (
        <div className="member-card-row">
          {members.map((m, i) => {
            const solo = data?.dir.byId.get(m.memberId)?.performer ?? 0;
            return (
              <button
                key={m.memberId}
                className={`member-card ${m.leftAt ? "former" : ""}`}
                style={{ ["--n" as string]: i }}
                onClick={() => onOpen(m.memberId)}
              >
                <Avatar path={data?.dir.byId.get(m.memberId)?.imagePath || m.memberImage} name={m.memberName} size={36} />
                <span>
                  <b>{m.memberName}</b>
                  <small>
                    {[m.part, spanOf(m.joined, m.leftAt), solo > 0 ? `个人 ${solo} 首 →` : ""].filter(Boolean).join(" · ")}
                  </small>
                </span>
              </button>
            );
          })}
        </div>
      )}

      {editing && (
        <div className="member-edit-list">
          {members.map((m) => (
            <MemberRow
              key={m.memberId}
              initial={m}
              onSave={(v) => run(() => api.membershipPut({ bandId, memberId: m.memberId, ...v }))}
              onRemove={() => run(() => api.membershipReject(bandId, m.memberId))}
            />
          ))}
          <MemberRow
            key={`new-${members.length}`}
            initial={null}
            onSave={(v) => run(() => api.membershipPut({ bandId, memberName: v.name, part: v.part, joined: v.joined, leftAt: v.leftAt }))}
          />
          <p className="muted small">
            日期写年份就行（2019，或 2019-05-27）；现在还在的，「离开」空着。改过的都记成手动，以后刮削不会盖掉。
          </p>
        </div>
      )}

      {suggestions.length > 0 && (
        <div className="member-suggestions">
          {suggestions.map((s) => (
            <div className="suggestion" key={s.memberId}>
              <Avatar path={data?.dir.byId.get(s.memberId)?.imagePath} name={s.memberName} size={26} />
              <span>
                <b>{s.memberName}</b> 可能是成员
                <small>
                  他写的 {s.written} 首（不算自己唱的）里有 {s.shared} 首给了这个乐队
                </small>
              </span>
              <button className="link-btn" onClick={() => void run(() => api.membershipConfirm(bandId, s.memberId))}>
                是成员
              </button>
              <button className="link-btn" onClick={() => void run(() => api.membershipReject(bandId, s.memberId))}>
                不是
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

interface RowValue {
  name: string;
  part: string;
  joined: string;
  leftAt: string;
}

function MemberRow({
  initial,
  onSave,
  onRemove,
}: {
  initial: Membership | null;
  onSave: (value: RowValue) => Promise<void>;
  onRemove?: () => Promise<void>;
}) {
  const [value, setValue] = useState<RowValue>({
    name: initial?.memberName ?? "",
    part: initial?.part ?? "",
    joined: initial?.joined ?? "",
    leftAt: initial?.leftAt ?? "",
  });
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [busy, setBusy] = useState(false);
  const changed =
    initial === null
      ? value.name.trim() !== ""
      : value.part !== initial.part || value.joined !== initial.joined || value.leftAt !== initial.leftAt;
  const datesOk = validDate(value.joined.trim()) && validDate(value.leftAt.trim());
  const set = (k: keyof RowValue) => (e: React.ChangeEvent<HTMLInputElement>) => setValue({ ...value, [k]: e.target.value });
  const save = async () => {
    setBusy(true);
    await onSave({ name: value.name.trim(), part: value.part.trim(), joined: value.joined.trim(), leftAt: value.leftAt.trim() });
    setBusy(false);
    if (initial === null) setValue({ name: "", part: "", joined: "", leftAt: "" });
  };

  return (
    <div className={`member-edit ${initial === null ? "new" : ""}`}>
      {initial === null ? (
        <input className="search" placeholder="新成员的名字" value={value.name} onChange={set("name")} />
      ) : (
        <b className="member-edit-name">{initial.memberName}</b>
      )}
      <input className="search" placeholder="分工（主唱、吉他…）" value={value.part} onChange={set("part")} />
      <input className={`search date ${validDate(value.joined.trim()) ? "" : "bad"}`} placeholder="加入" value={value.joined} onChange={set("joined")} />
      <input className={`search date ${validDate(value.leftAt.trim()) ? "" : "bad"}`} placeholder="离开" value={value.leftAt} onChange={set("leftAt")} />
      <button className="link-btn" disabled={!changed || !datesOk || busy} onClick={() => void save()}>
        {initial === null ? "添加" : "保存"}
      </button>
      {onRemove &&
        (confirmRemove ? (
          <span className="confirm-delete">
            不是成员？
            <button className="link-btn danger" onClick={() => void onRemove()}>
              确认
            </button>
            <button className="link-btn" onClick={() => setConfirmRemove(false)}>
              取消
            </button>
          </span>
        ) : (
          <button className="link-btn danger" onClick={() => setConfirmRemove(true)} title="记成「不是成员」，以后刮削也不会加回来">
            移除
          </button>
        ))}
    </div>
  );
}
