/**
 * 设置页上 ffmpeg 那一行的文案。
 *
 * ffmpeg 是可选依赖：0.2.x 的安装包不带它，后端只在 PATH 和语料库目录里找。
 * 缺了它，变调和 Anki 音频片段两样不可用。以前界面上只有一句「不支持变调」，
 * 既不说为什么，也不说 Anki 那一样，更不说放到哪儿才找得到。
 */

import type { HealthReport } from "./api";

export interface FfmpegNote {
  found: boolean;
  /** 卡片右边那一小句 */
  status: string;
  /** 卡片下面的说明 */
  detail: string;
}

export function ffmpegNote(health: Pick<HealthReport, "ffmpegPath" | "ffmpegExpected">): FfmpegNote {
  if (health.ffmpegPath !== null) {
    return {
      found: true,
      status: "已找到",
      detail: `用的是 ${health.ffmpegPath}。变调和 Anki 音频片段靠它。`,
    };
  }
  return {
    found: false,
    status: "没找到",
    detail:
      `变调和 Anki 音频片段不可用，其余功能不受影响。` +
      `把 ffmpeg 放进 PATH 里的任一目录，或者放到 ${health.ffmpegExpected}，重启应用后生效。`,
  };
}
