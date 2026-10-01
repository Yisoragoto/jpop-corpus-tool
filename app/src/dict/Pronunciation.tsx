/*
 * 音高显示。
 *
 * Yomitan `ext/js/display/pronunciation-generator.js` 的 React 移植
 * （Copyright (C) 2023-2026 Yomitan Authors，GPL-3.0-or-later）。类名和 Yomitan 一致，样式见 dict.css。
 */

import type { ReactNode } from "react";

import { getDownstepPositions, getKanaDiacriticInfo, getKanaMorae, isMoraPitchHigh } from "./japanese";

interface PitchProps {
  reading: string;
  positions: number | string;
}

/** 假名上画高音线，降调处画竖线（Yomitan 的「文本」样式）。 */
export function PitchText({
  reading,
  positions,
  nasalPositions = [],
  devoicePositions = [],
}: PitchProps & { nasalPositions?: number[]; devoicePositions?: number[] }) {
  const nasal = new Set(nasalPositions);
  const devoice = new Set(devoicePositions);
  return (
    <span className="pronunciation-text">
      {getKanaMorae(reading).map((mora, i) => {
        const high = isMoraPitchHigh(i, positions);
        const highNext = isMoraPitchHigh(i + 1, positions);
        const isNasal = nasal.has(i + 1);
        const isDevoice = devoice.has(i + 1);
        const characters = [...mora];

        let characterNodes: ReactNode = characters.map((c, j) => (
          <span key={j} className="pronunciation-character">
            {c}
          </span>
        ));
        const first = characters[0];
        if (isNasal && first !== undefined) {
          // 鼻浊音：浊音假名换成清音 + 合成用半浊点
          const info = getKanaDiacriticInfo(first);
          characterNodes = (
            <>
              <span className="pronunciation-character-group">
                <span className="pronunciation-character" data-original-text={info !== null ? first : undefined}>
                  {info !== null ? info.character : first}
                </span>
                <span className="pronunciation-nasal-diacritic">{"゚"}</span>
                <span className="pronunciation-nasal-indicator" />
              </span>
              {characters.slice(1).map((c, j) => (
                <span key={j} className="pronunciation-character">
                  {c}
                </span>
              ))}
            </>
          );
        }

        return (
          <span
            key={i}
            className="pronunciation-mora"
            data-position={i}
            data-pitch={high ? "high" : "low"}
            data-pitch-next={highNext ? "high" : "low"}
            data-devoice={isDevoice ? "true" : undefined}
            data-nasal={isNasal ? "true" : undefined}
            data-original-text={isNasal && first !== undefined && getKanaDiacriticInfo(first) !== null ? mora : undefined}
          >
            {characterNodes}
            {isDevoice && <span className="pronunciation-devoice-indicator" />}
            <span className="pronunciation-mora-line" />
          </span>
        );
      })}
    </span>
  );
}

/** 折线图：每个音拍一个点，降调前的点画成空心，尾巴虚线指向后接助词的高低。 */
export function PitchGraph({ reading, positions }: PitchProps) {
  const morae = getKanaMorae(reading);
  const count = morae.length;
  const viewBox = `0 0 ${50 * (count + 1)} 100`;
  if (count <= 0) {
    return <svg className="pronunciation-graph" viewBox={viewBox} focusable="false" />;
  }

  const points: string[] = [];
  const dots: ReactNode[] = [];
  for (let i = 0; i < count; ++i) {
    const high = isMoraPitchHigh(i, positions);
    const highNext = isMoraPitchHigh(i + 1, positions);
    const x = i * 50 + 25;
    const y = high ? 25 : 75;
    if (high && !highNext) {
      dots.push(<circle key={`${i}a`} className="pronunciation-graph-dot-downstep1" cx={x} cy={y} r="15" />);
      dots.push(<circle key={`${i}b`} className="pronunciation-graph-dot-downstep2" cx={x} cy={y} r="5" />);
    } else {
      dots.push(<circle key={i} className="pronunciation-graph-dot" cx={x} cy={y} r="15" />);
    }
    points.push(`${x} ${y}`);
  }
  const tailX = count * 50 + 25;
  const tailY = isMoraPitchHigh(count, positions) ? 25 : 75;
  const tail = [points[count - 1], `${tailX} ${tailY}`];

  return (
    <svg className="pronunciation-graph" viewBox={viewBox} focusable="false">
      <path className="pronunciation-graph-line" d={`M${points.join(" L")}`} />
      <path className="pronunciation-graph-line-tail" d={`M${tail.join(" L")}`} />
      {dots}
      <path
        className="pronunciation-graph-triangle"
        d="M0 13 L15 -13 L-15 -13 Z"
        transform={`translate(${tailX},${tailY})`}
      />
    </svg>
  );
}

/** `[2]` 这种降调位置记号。 */
export function DownstepNotation({ positions }: { positions: number | string }) {
  const downsteps = typeof positions === "string" ? getDownstepPositions(positions) : positions;
  const text = String(downsteps);
  return (
    <span className="pronunciation-downstep-notation" data-downstep-position={text}>
      <span className="pronunciation-downstep-notation-prefix">[</span>
      <span className="pronunciation-downstep-notation-number">{text}</span>
      <span className="pronunciation-downstep-notation-suffix">]</span>
    </span>
  );
}
