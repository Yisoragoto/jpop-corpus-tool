/** 一个数字 + 一个标签。顶栏和各页的统计块共用。 */
export function Stat({
  label,
  value,
  align = "end",
}: {
  label: string;
  value: number | string;
  align?: "start" | "end";
}) {
  return (
    <div className="stat" style={{ alignItems: align === "start" ? "flex-start" : "flex-end" }}>
      <b>{typeof value === "number" ? value.toLocaleString() : value}</b>
      <span>{label}</span>
    </div>
  );
}
