/**
 * 释义的纯文本：折叠后的一行预览、纯文本释义里的换行。
 */

import type { GlossaryEntry, StructuredContent } from "./api";

const LINE_BREAK = /<br\s*\/?>/i;

/**
 * 有的词典（比如明鏡日汉双解）在纯文本释义里写了 `<br>`。Yomitan 把纯文本原样显示，
 * 会把它露出来；这里当换行。只认 `<br>` 这一个标签，其余照旧按文本显示，不当 HTML 解析。
 */
export function splitLineBreaks(text: string): string[] {
  return text.split(LINE_BREAK);
}

function structuredText(content: StructuredContent | undefined): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) return content.map(structuredText).join("");
  if (typeof content !== "object" || content === null) return "";
  switch (content.tag) {
    // 振假名不进预览，免得「今更いまさら」这样读音和汉字挤在一起
    case "rt":
    case "rp":
    case "img":
      return "";
    case "br":
      return " ";
    default:
      return structuredText(content.content);
  }
}

export function glossaryPlainText(entry: GlossaryEntry): string {
  if (typeof entry === "string") return splitLineBreaks(entry).join(" ");
  if (entry.type === "structured-content") return structuredText(entry.content);
  return typeof entry.description === "string" ? entry.description : "［图］";
}

/** 折叠时显示的一行：前几条释义的纯文本，压掉多余空白。 */
export function previewText(entries: GlossaryEntry[], maxLength = 80): string {
  const text = entries.map(glossaryPlainText).join(" / ").replace(/\s+/g, " ").trim();
  return text.length > maxLength ? `${text.slice(0, maxLength)}…` : text;
}
