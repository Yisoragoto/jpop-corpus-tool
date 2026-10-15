/** ISO 时间 → 「3 分钟前」。收听记录看的是「多久以前」，不是具体几点。 */
export function formatRelative(iso: string, now = Date.now()): string {
  const then = Date.parse(iso);
  if (!Number.isFinite(then)) return "";
  const seconds = Math.max(0, (now - then) / 1000);
  if (seconds < 60) return "刚刚";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟前`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} 小时前`;
  if (seconds < 86400 * 60) return `${Math.floor(seconds / 86400)} 天前`;
  return `${Math.floor(seconds / (86400 * 30))} 个月前`;
}
