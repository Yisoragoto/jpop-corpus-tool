/**
 * 全屏歌词的 AMLL 版（Apple Music 风格：逐字发光、弹簧滚动、振假名跟着唱）。
 *
 * 点击：AMLL 补丁把被点中的词元素放进 `line-click` 事件（`wordElement`，带 `data-char-start`），
 * 这里再用点击坐标算出点中的是这个词里的第几个字（`charAtClick`），交给 `onWord`：
 * 那个字落在一个能查的词上就查词，否则（点在标点、空白、行的空处）跳到那一句。
 * 查过的词描一圈（`.amll-picked`），和经典视图里高亮查过的词一个意思。
 *
 * 时钟和逐字点亮一样走 `advanceClock`：轮询来的位置是 10Hz 的，AMLL 每帧要一个平滑的时间。
 */

import { useEffect, useRef, useState } from "react";

import type { KeyedAmllLine } from "../amllLines";
import { charAtClick } from "../amllLines";
import { tr } from "../i18n";
import { advanceClock, type ClockState } from "../useKaraoke";

interface Props {
  lines: KeyedAmllLine[];
  positionSec: number;
  playing: boolean;
  rate: number;
  /** 点中了第 lineKey 行的第 charIndex 个字。返回 [起, 止) 表示查了这个词（用来描框），null 表示没查 */
  onWord: (lineKey: number, charIndex: number) => [number, number] | null;
  /** 点在行上但不是能查的词：跳到这一句 */
  onLine: (lineKey: number) => void;
  onFail: () => void;
}

/** 和轮询差这么多就算拖了进度条：告诉 AMLL 这是一次跳转，它会直接定位而不是慢慢滚 */
const SEEK_SEC = 0.6;

type Player = {
  getElement: () => HTMLElement;
  setLyricLines: (lines: KeyedAmllLine[], initialTime?: number) => void;
  setCurrentTime: (ms: number, isSeek?: boolean) => void;
  update: (delta?: number) => void;
  pause: () => void;
  resume: () => void;
  dispose: () => void;
  addEventListener: (type: string, listener: (e: Event) => void) => void;
};
type LineClick = MouseEvent & { line: { getLine: () => KeyedAmllLine }; wordElement: Element | null };

export function AmllLyrics(props: Props) {
  const box = useRef<HTMLDivElement>(null);
  const [player, setPlayer] = useState<Player | null>(null);
  const latest = useRef(props);
  latest.current = props;

  useEffect(() => {
    let alive = true;
    let created: Player | null = null;
    void import("../amll").then((amll) => {
      if (!alive || box.current === null) return;
      created = new amll.LyricPlayer() as unknown as Player;
      box.current.appendChild(created.getElement());
      created.addEventListener("line-click", (event) => {
        const e = event as LineClick;
        const lineKey = e.line.getLine().lineKey;
        const word = e.wordElement;
        if (word !== null) {
          const range = document.caretRangeFromPoint?.(e.clientX, e.clientY) ?? null;
          const at = charAtClick(word, range);
          if (at !== null) {
            const picked = latest.current.onWord(lineKey, at);
            if (picked !== null) {
              markPicked(word, picked);
              return;
            }
          }
        }
        latest.current.onLine(lineKey);
      });
      setPlayer(created);
    }, () => alive && latest.current.onFail());
    return () => {
      alive = false;
      created?.dispose();
    };
  }, []);

  useEffect(() => {
    player?.setLyricLines(props.lines, Math.round(latest.current.positionSec * 1000));
    player?.update();
  }, [player, props.lines]);

  const polled = useRef({ pos: props.positionSec, at: performance.now() });
  useEffect(() => {
    polled.current = { pos: props.positionSec, at: performance.now() };
  }, [props.positionSec]);

  useEffect(() => {
    if (player === null) return;
    if (props.playing) player.resume();
    else player.pause();
    let frame = 0;
    let last = -1;
    let clock: ClockState | null = null;
    const onFrame = (now: number) => {
      const before = clock?.sec ?? null;
      clock = advanceClock(clock, polled.current, performance.now(), props.playing, props.rate);
      const jumped = before === null || Math.abs(clock.sec - before) > SEEK_SEC;
      player.setCurrentTime(Math.round(clock.sec * 1000), jumped);
      player.update(last < 0 ? 0 : now - last);
      last = now;
      frame = requestAnimationFrame(onFrame);
    };
    frame = requestAnimationFrame(onFrame);
    return () => cancelAnimationFrame(frame);
  }, [player, props.playing, props.rate]);

  return <div className="amll-lyrics" ref={box} lang="ja" aria-label={tr("歌词（点词查词，点空白处跳到那一句）")} />;
}

/** 给查过的那个词描框：同一行里 data-char-start 落在 [起, 止) 的词元素 */
function markPicked(word: Element, [start, end]: [number, number]) {
  const root = word.closest(".amll-lyric-player") ?? document;
  for (const el of root.querySelectorAll(".amll-picked")) el.classList.remove("amll-picked");
  const line = word.parentElement?.closest("[class*='lyricMainLine'],[class*='lyricLine']") ?? word.parentElement;
  for (const el of line?.querySelectorAll<HTMLElement>("[data-char-start]") ?? []) {
    const at = Number(el.dataset.charStart);
    if (at >= start && at < end) el.classList.add("amll-picked");
  }
}
