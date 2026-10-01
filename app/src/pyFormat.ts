/**
 * 按 Python 的规则把数字和表格写成文本。
 *
 * 导出的文件要和 PyQt 版逐字节一样，界面上显示的数字也要和导出的对得上，
 * 而 JS 自带的格式化有两处不同：
 * - `toFixed` 正好一半时远离零进位，Python 的 `:.Nf` 取偶（`f"{0.125:.2f}"` 是 0.12）；
 * - `String(60)` 是 "60"，Python 的 `str(60.0)` 是 "60.0"。
 *
 * `testdata/python_formats.json` 是 Python 真实写出的结果，测试逐条对。
 */

/** Python 的 `f"{x:.{digits}f}"` */
export function pyFixed(x: number, digits: number): string {
  const rounded = x.toFixed(digits);
  if (!Number.isFinite(x) || Math.abs(x) >= 1e21) return rounded;
  // toFixed(100) 是二进制真值的精确十进制展开（这些量级的双精度数小数位不超过 100 位）
  const exact = Math.abs(x).toFixed(100);
  const dot = exact.indexOf(".");
  const rest = exact.slice(dot + 1 + digits);
  if (!/^50*$/.test(rest)) return rounded;
  // 正好一半：toFixed 往远离零进了位；截掉的那一位是偶数时 Python 不进位
  const kept = exact.slice(0, digits === 0 ? dot : dot + 1 + digits);
  const last = Number(kept[kept.length - 1]);
  if (last % 2 === 1) return rounded;
  return (x < 0 ? "-" : "") + kept;
}

/** Python 的 `f"{n:,}"`（整数） */
export function pyGrouped(n: number): string {
  const sign = n < 0 ? "-" : "";
  const digits = String(Math.abs(Math.trunc(n)));
  return sign + digits.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

/** Python 的 `str(float)`：最短往返的数字，整数也带 `.0`，很小或很大时用 `1e-05` 这种指数写法 */
export function pyFloatRepr(x: number): string {
  if (Number.isNaN(x)) return "nan";
  if (!Number.isFinite(x)) return x > 0 ? "inf" : "-inf";
  const abs = Math.abs(x);
  if (abs !== 0 && (abs < 1e-4 || abs >= 1e16)) {
    // JS 的 "5e-5" / "1.5e+21" → Python 的 "5e-05" / "1.5e+21"
    const [mantissa, exponent = "+0"] = x.toExponential().split("e");
    const sign = exponent[0] === "-" ? "-" : "+";
    const power = exponent.replace(/^[+-]/, "").padStart(2, "0");
    return `${mantissa}e${sign}${power}`;
  }
  const text = String(x);
  return Number.isInteger(x) ? `${Object.is(x, -0) ? "-0" : text}.0` : text;
}

/** `csv.writer` 默认方言（excel）的一个字段：含逗号、引号、回车或换行才加引号，引号写两遍 */
function csvField(value: string): string {
  return /[",\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

/** `csv.writer` 的一行，行尾 `\r\n`。`null` 写成空字段（Python 的 None） */
export function csvRow(values: (string | number | null)[]): string {
  return (
    values
      .map((v) => (v === null ? "" : typeof v === "number" ? csvField(pyFloatRepr(v)) : csvField(v)))
      .join(",") + "\r\n"
  );
}

/** utf-8-sig 写出的文件开头那个 BOM。写成码位，源码里不放看不见的字符 */
const BOM = String.fromCharCode(0xfeff);

export interface KwicCsvHit {
  artist: string;
  title: string;
  timeSec: number | null;
  left: string;
  keyword: string;
  right: string;
  text: string;
}

/**
 * 检索结果的 CSV，照 Python `export_csv`：utf-8-sig（开头一个 BOM）、七列、时刻是浮点数原样。
 * 左右语境写完整的，不按界面上的显示宽度截断。
 */
export function kwicCsv(hits: KwicCsvHit[]): string {
  let out = BOM + csvRow(["歌手", "曲名", "時刻(秒)", "左文脈", "キーワード", "右文脈", "全文"]);
  for (const h of hits) {
    out += csvRow([h.artist, h.title, h.timeSec, h.left, h.keyword, h.right, h.text]);
  }
  return out;
}
