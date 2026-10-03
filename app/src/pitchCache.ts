/**
 * 设置页上「变调缓存」那一行要用的东西：上限的可选值，和「已用多少」的文案。
 *
 * 变调过的歌存一份 WAV 在语料库的 `output/pitch_cache/` 里，一首 4 分钟的歌约 45 MB。
 * 超过上限时后端从最久没放的删起（见 `jp_audio::pitch::enforce_limit`）。
 */

/** 上限的可选值（MB）。默认 1536，和后端的默认一致 */
export const PITCH_CACHE_LIMITS_MB = [512, 1024, 1536, 2048, 3072, 5120, 10240, 20480] as const;

/** 后端肯收的范围，和 `commands.rs` 的 `PITCH_CACHE_LIMIT_MB` 一致 */
export const PITCH_CACHE_LIMIT_RANGE = [256, 102400] as const;

const MB = 1024 * 1024;

/** 1536 → "1.5 GB"，512 → "512 MB" */
export function limitLabel(mb: number): string {
  if (mb < 1024) return `${mb} MB`;
  const gb = mb / 1024;
  return `${Number.isInteger(gb) ? gb : gb.toFixed(1)} GB`;
}

/** 字节数写成人看的：不到 1 GB 用 MB，往上用 GB */
export function sizeLabel(bytes: number): string {
  if (bytes < 1024 * MB) return `${Math.round(bytes / MB)} MB`;
  return `${(bytes / (1024 * MB)).toFixed(1)} GB`;
}

/** 卡片右边那一小句：「已用 312 MB · 7 个」。一个缓存文件是一首歌的一个调 */
export function usageLabel(status: { files: number; bytes: number }): string {
  if (status.files === 0) return "还没有缓存";
  return `已用 ${sizeLabel(status.bytes)} · ${status.files} 个`;
}
