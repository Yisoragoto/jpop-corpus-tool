/** ISO 时间 → 「3 分钟前」。收听记录看的是「多久以前」，不是具体几点。 */
import { tr } from "./i18n";

export function formatRelative(iso: string, now = Date.now()): string {
  const then = Date.parse(iso);
  if (!Number.isFinite(then)) return "";
  const seconds = Math.max(0, (now - then) / 1000);
  if (seconds < 60) return tr("刚刚");
  if (seconds < 3600) return tr("{floor} 分钟前", { floor: Math.floor(seconds / 60) });
  if (seconds < 86400) return tr("{floor} 小时前", { floor: Math.floor(seconds / 3600) });
  if (seconds < 86400 * 60) return tr("{floor} 天前", { floor: Math.floor(seconds / 86400) });
  return tr("{floor} 个月前", { floor: Math.floor(seconds / (86400 * 30)) });
}
