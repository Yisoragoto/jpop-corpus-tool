/**
 * 词典装上 / 迁移完之后，后端会顺手给「有歌词、没分词」的歌补分词。
 * 这里把结果变成一句话，接在「装好了」后面。
 *
 * 没有要补的就什么都不说——大多数时候是这样，多一句「补了 0 首」只是噪音。
 */

import type { Tokenized } from "./api";

export function backfillNote(tokenized: Tokenized, error: string): string {
  if (error !== "") {
    return `顺手补分词没成功（库没被改动）：${error}。可以到「曲库维护 → 补齐缺失分词」再试。`;
  }
  if (tokenized.songs === 0) return "";
  return `顺手给 ${tokenized.songs} 首没分词的歌补上了分词（${tokenized.tokens.toLocaleString()} 个词），那几首现在能查词、有振假名了。`;
}
