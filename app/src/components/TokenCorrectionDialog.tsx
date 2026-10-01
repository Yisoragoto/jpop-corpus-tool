/**
 * 分词校正编辑器。
 *
 * 功能和 Python 版的分词校正对话框一致：直接改表层形 / 词元 / 词性，
 * 选中相邻几个词合并，选中一个词按字符位置拆开。
 *
 * 和 Python 版不同的是撤销：那边的「重置」只是把原始分词填回表格，存下来仍是一条校正；
 * 这里的「撤销校正」直接回到分词器的结果并删掉校正记录（见 `jp_corpus::corrections::revert`）。
 * 确认放在按钮上点第二下，不弹窗。
 */

import { useEffect, useMemo, useState } from "react";

import { api, POS_LABELS, UPOS_TAGS, type CorrectionView, type TokenEdit } from "../api";
import {
  charCount,
  cleanTokens,
  isContiguous,
  matchesLine,
  mergeTokens,
  sameTokens,
  splitToken,
} from "../corrections";

const KNOWN_POS = new Set<string>(UPOS_TAGS);

interface Props {
  utteranceId: number;
  onClose: () => void;
  /** 保存或撤销成功之后调用。调用方负责重新检索。 */
  onChanged: () => void;
  onError: (message: string) => void;
}

export function TokenCorrectionDialog({ utteranceId, onClose, onChanged, onError }: Props) {
  const [view, setView] = useState<CorrectionView | null>(null);
  const [tokens, setTokens] = useState<TokenEdit[]>([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [splitAt, setSplitAt] = useState(1);
  const [busy, setBusy] = useState(false);
  const [confirmRevert, setConfirmRevert] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .tokenCorrection(utteranceId)
      .then((v) => {
        if (!alive) return;
        if (!v) {
          onError("这一行已经不在库里了");
          onClose();
          return;
        }
        setView(v);
        setTokens(v.tokens);
      })
      .catch((err: unknown) => onError(err instanceof Error ? err.message : String(err)));
    return () => {
      alive = false;
    };
    // 只跟着行变：回调由调用方用 useCallback 稳定下来
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [utteranceId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const cleaned = useMemo(() => cleanTokens(tokens), [tokens]);
  const dirty = view !== null && !sameTokens(cleaned, view.tokens);
  const aligned = view === null || matchesLine(cleaned, view.text);

  const single = selected.length === 1 ? (selected[0] ?? null) : null;
  const singleToken = single !== null ? tokens[single] : undefined;
  const singleLen = singleToken ? charCount(singleToken.surface) : 0;
  const canMerge = isContiguous(selected);
  const canSplit = singleToken !== undefined && singleLen >= 2;

  const toggle = (i: number) =>
    setSelected((s) => (s.includes(i) ? s.filter((x) => x !== i) : [...s, i].sort((a, b) => a - b)));

  const edit = (i: number, field: keyof TokenEdit, value: string) =>
    setTokens((ts) => ts.map((t, j) => (j === i ? { ...t, [field]: value } : t)));

  const merge = () => {
    const first = selected[0];
    setTokens((ts) => mergeTokens(ts, selected));
    setSelected(first !== undefined ? [first] : []);
  };

  const split = () => {
    if (single === null) return;
    setTokens((ts) => splitToken(ts, single, splitAt));
    setSelected([]);
  };

  const save = async () => {
    setBusy(true);
    try {
      await api.saveTokenCorrection(utteranceId, cleaned);
      onChanged();
      onClose();
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const revert = async () => {
    if (!confirmRevert) {
      setConfirmRevert(true);
      return;
    }
    setBusy(true);
    try {
      await api.revertTokenCorrection(utteranceId);
      onChanged();
      onClose();
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="palette-backdrop" onMouseDown={onClose}>
      <div
        className="palette tc-dialog"
        role="dialog"
        aria-label="分词校正"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="tc-head">
          <div className="tc-text">{view?.text ?? "加载中…"}</div>
          {view && (
            <div className="muted small">
              {view.artist} · {view.title}
              {view.corrected && " · 已校正过"}
            </div>
          )}
        </div>

        <div className="tc-table">
          <div className="tc-row tc-header">
            <span />
            <span>表层形</span>
            <span>词元</span>
            <span>词性</span>
          </div>
          {tokens.map((t, i) => (
            <div key={i} className={`tc-row ${selected.includes(i) ? "on" : ""}`}>
              <button className="tc-pick" onClick={() => toggle(i)} title="选中 / 取消选中">
                {i + 1}
              </button>
              <input value={t.surface} onChange={(e) => edit(i, "surface", e.target.value)} />
              <input value={t.lemma} onChange={(e) => edit(i, "lemma", e.target.value)} />
              <select value={t.pos} onChange={(e) => edit(i, "pos", e.target.value)}>
                {!KNOWN_POS.has(t.pos) && <option value={t.pos}>{t.pos || "（空）"}</option>}
                {UPOS_TAGS.map((p) => (
                  <option key={p} value={p}>
                    {p}
                    {POS_LABELS[p] ? ` · ${POS_LABELS[p]}` : ""}
                  </option>
                ))}
              </select>
            </div>
          ))}
        </div>

        <div className="tc-actions">
          <button className="tc-btn" disabled={!canMerge || busy} onClick={merge}>
            合并选中的词
          </button>
          <span className="muted small">在第</span>
          <input
            type="number"
            className="tc-split"
            min={1}
            max={Math.max(1, singleLen - 1)}
            value={splitAt}
            disabled={!canSplit}
            onChange={(e) => setSplitAt(Number(e.target.value) || 1)}
          />
          <span className="muted small">个字后</span>
          <button
            className="tc-btn"
            disabled={!canSplit || busy || splitAt < 1 || splitAt > singleLen - 1}
            onClick={split}
          >
            拆开
          </button>
          <span className="muted small tc-hint">点左边的序号选词</span>
        </div>

        {!aligned && (
          <p className="warn small tc-note">
            词拼起来和原句对不上。可以保存，但 KWIC 的左右语境会退回按词拼接。
          </p>
        )}

        <div className="tc-footer">
          {view?.corrected ? (
            <button className="tc-btn danger" disabled={busy} onClick={() => void revert()}>
              {confirmRevert ? "再点一次，撤销校正" : "撤销校正"}
            </button>
          ) : (
            <button
              className="tc-btn"
              disabled={!dirty || busy}
              onClick={() => view && setTokens(view.tokens)}
            >
              放弃修改
            </button>
          )}
          <span className="spacer" />
          <button className="tc-btn" onClick={onClose}>
            取消
          </button>
          <button
            className="tc-btn primary"
            disabled={!dirty || cleaned.length === 0 || busy}
            onClick={() => void save()}
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
