/**
 * 英文。键是中文原文（见 `i18n.ts`），按所在文件分在 en1–en5 里。
 * 加了新的中文字面量之后跑 `npm test`：`i18n.test.ts` 会列出还缺英文的那些。
 */

import type { Entry } from "../i18n";
import { en1 } from "./en1";
import { en2 } from "./en2";
import { en3 } from "./en3";
import { en4 } from "./en4";
import { en5 } from "./en5";

export const en: Record<string, Entry> = { ...en1, ...en2, ...en3, ...en4, ...en5 };
