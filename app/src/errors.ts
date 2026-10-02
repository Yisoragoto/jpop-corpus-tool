/**
 * 把「随便什么被抛出来的东西」变成一句能给用户看的话。
 *
 * **为什么要单独一个函数**：JS 里 `throw` 什么都行。实际会碰到的至少有四种：
 *
 * * `Error`——正常情况，`api.call()` 把 Rust 的 `CommandError` 包成了它；
 * * `{ message: "..." }`——没走 `api.call()` 的地方直接拿到的 IPC 原始对象；
 * * 字符串——老代码和一些库；
 * * `undefined` / `null` / 随便一个对象——`String(err)` 对它们会给出
 *   `"undefined"` 或者 `"[object Object]"`，贴到界面上等于什么都没说。
 *
 * 各个页面原来各写各的 `String((e as { message?: string })?.message ?? e)`，
 * 第四种就是这么漏出去的。
 */
export function messageOf(err: unknown): string {
  if (err instanceof Error && err.message !== "") return err.message;
  if (typeof err === "string" && err !== "") return err;
  if (typeof err === "object" && err !== null) {
    const message = (err as { message?: unknown }).message;
    if (typeof message === "string" && message !== "") return message;
  }
  // 到这儿说明抛出来的东西没带任何可读信息。**不要给 "[object Object]"**：
  // 那句话既没告诉用户发生了什么，也没告诉我该去查哪儿。
  return "出了点问题，但没有带上原因（详情见设置 → 系统 → 导出诊断信息）";
}

/** 界面崩掉时那块白屏上要写的话。抽出来是为了能单测——组件本身没有逻辑。 */
export function crashNotice(err: unknown): { title: string; detail: string } {
  return {
    title: "界面出错了",
    detail: messageOf(err),
  };
}
