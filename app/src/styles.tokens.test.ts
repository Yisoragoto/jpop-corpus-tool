import { expect, it } from "vitest";
import css from "./styles.css?raw";
import baseline from "./styles.tokens.baseline.txt?raw";


/** 行号只供定位：前面增删一行不该让所有旧颜色都算新增；同一行重复出现仍按次数计数。 */
function violations(source: string): string[] {
  const withoutComments = source.replace(/\/\*[\s\S]*?\*\//g, (s) => s.replace(/[^\n]/g, " "));
  const root = /:root\s*\{[^{}]*\}/g;
  const outside = withoutComments.replace(root, (s) => s.replace(/[^\n]/g, " "));
  return outside.split("\n").flatMap((line, i) =>
    [...line.matchAll(/#[\da-f]{6}\b|#[\da-f]{3}\b|\brgba?\([^)]*\)/gi)].map(() => `${i + 1}: ${source.split("\n")[i]!.trim()}`),
  );
}

it("root 外的硬编码色值不能超出存量基线", () => {
  expect(css.length, "不能把 Vitest 的空 CSS mock 当成真实样式检查").toBeGreaterThan(100);
  const counts = new Map<string, number>();
  const original = (s: string) => s.replace(/^\d+:\s*/, "").trim();
  for (const line of baseline.split(/\r?\n/).filter(Boolean)) {
    const key = original(line);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  const added = violations(css).filter((line) => {
    const key = original(line);
    const left = counts.get(key) ?? 0;
    counts.set(key, left - 1);
    return left <= 0;
  });
  expect(added, "新增颜色请使用 :root 中的 token；基线只允许减少").toEqual([]);
});

it("守卫能识别新增颜色、忽略 token 和注释", () => {
  expect(violations(":root { --danger: #f00; }\n.page { color: #ff0000; }")).toHaveLength(1);
  expect(violations("/* color: #fff */\n.page { color: var(--danger); }")).toEqual([]);
});
