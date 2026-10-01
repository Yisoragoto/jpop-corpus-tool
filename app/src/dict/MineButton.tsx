/**
 * 查词结果上的「＋ 制卡」按钮。
 *
 * - Anki 里已经有这个词：显示「已有卡」，点了在 Anki 里打开；
 * - 制卡成功：「✓ 已制卡」，点了打开刚加的那张；
 * - Anki 没开、没有 Lapis 笔记类型：按钮灰掉，悬停看原因。
 */

import { useEffect, useState } from "react";

import type { TermDictionaryEntry } from "./api";
import { artistSubdeck, mineApi, useMineSettings, type MineContext } from "./mine";

type State =
  | { kind: "idle" }
  | { kind: "adding" }
  | { kind: "added"; noteId: number; deck: string; deckCreated: boolean; notes: string[]; warnings: string[] }
  | { kind: "error"; message: string };

interface Props {
  entry: TermDictionaryEntry;
  context: MineContext;
  /** Anki 里已有的笔记；undefined 表示还没查到 */
  existing: number[] | undefined;
  /** Anki 不可用的原因；空串表示可用 */
  unavailable: string;
  onError: (message: string) => void;
}

function errorMessage(e: unknown): string {
  return String((e as { message?: string })?.message ?? e);
}

export function MineButton({ entry, context, existing, unavailable, onError }: Props) {
  const settings = useMineSettings();
  const [state, setState] = useState<State>({ kind: "idle" });
  const [duplicate, setDuplicate] = useState<number[] | null>(null);
  const headword = entry.headwords[0];

  // 换了一次查词就重置
  useEffect(() => {
    setState({ kind: "idle" });
    setDuplicate(null);
  }, [context.lookupText, context.utteranceId, headword?.term, headword?.reading]);

  if (headword === undefined) return null;

  const browse = (ids: number[]) => {
    mineApi.browse(ids).catch((e: unknown) => onError(errorMessage(e)));
  };

  const existingIds = duplicate ?? (existing !== undefined && existing.length > 0 ? existing : null);
  const subdeck =
    settings.artistSubdeck && context.songId !== undefined ? artistSubdeck(settings.deck, context.artist ?? "") : null;
  if (state.kind === "added") {
    const problems = [...state.warnings, ...state.notes];
    return (
      <button
        className={problems.length > 0 ? "mine-btn done partial" : "mine-btn done"}
        onClick={() => browse([state.noteId])}
        title={[
          `已加入 Anki「${state.deck}」${state.deckCreated ? "（新建的子牌组）" : ""}，点击在 Anki 里打开`,
          ...problems,
        ].join("\n")}
      >
        {state.warnings.length > 0 ? "✓ 已制卡（有问题）" : "✓ 已制卡"}
      </button>
    );
  }
  if (existingIds !== null) {
    return (
      <button className="mine-btn existing" onClick={() => browse(existingIds)} title="Anki 里已经有这个词的卡，点击打开">
        已有卡
      </button>
    );
  }

  const mine = async () => {
    setState({ kind: "adding" });
    try {
      const outcome = await mineApi.mine(context, headword.term, headword.reading, settings);
      if (outcome.kind === "added") {
        const notes =
          outcome.skippedFields.length > 0
            ? [`这个笔记类型没有这些字段，内容没写进去：${outcome.skippedFields.join("、")}`]
            : [];
        setState({
          kind: "added",
          noteId: outcome.noteId,
          deck: outcome.deck,
          deckCreated: outcome.deckCreated,
          notes,
          warnings: outcome.warnings,
        });
        // 当前句没有音频这类问题要让人看见，不能只藏在悬停提示里
        if (outcome.warnings.length > 0) onError(`「${headword.term}」已制卡，但${outcome.warnings.join("；")}`);
      } else {
        setDuplicate(outcome.noteIds);
        setState({ kind: "idle" });
      }
    } catch (e) {
      const message = errorMessage(e);
      setState({ kind: "error", message });
      onError(message);
    }
  };

  return (
    <button
      className={state.kind === "error" ? "mine-btn failed" : "mine-btn"}
      disabled={state.kind === "adding" || unavailable !== ""}
      onClick={() => void mine()}
      title={
        unavailable !== ""
          ? unavailable
          : state.kind === "error"
            ? `制卡失败：${state.message}（点击重试）`
            : subdeck !== null
              ? `做成 ${settings.model} 卡，放进「${subdeck}」（没有这个子牌组就新建）`
              : `做成 ${settings.model} 卡，放进「${settings.deck}」`
      }
    >
      {state.kind === "adding" ? "制卡中…" : state.kind === "error" ? "重试制卡" : "＋ 制卡"}
    </button>
  );
}
