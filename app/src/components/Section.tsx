/**
 * 页面里的一块。**不是卡片**——只有一行标题，内容直接铺在页底色上。
 *
 * 原来每一块都套一个带边框的圆角盒子，一页下来就是十几个长短不一的方块，
 * 看着碎。现在靠标题和间距分块，边框留给真正需要「圈起来」的东西
 * （表单、提示、需要确认的操作）。
 */

import type { ReactNode } from "react";

interface Props {
  title: ReactNode;
  /** 标题右边的一句话，说明这一块是什么 */
  hint?: ReactNode;
  /** 标题最右边的操作，一般是「更多 →」 */
  action?: ReactNode;
  children: ReactNode;
  /** 内容为空时显示这句话，不给空白 */
  empty?: ReactNode;
  /** 传 false 就整块不渲染（该块没有内容且不值得提示时） */
  show?: boolean;
}

export function Section({ title, hint, action, children, empty, show = true }: Props) {
  if (!show) return null;
  return (
    <section className="section">
      <header className="section-head">
        <h2>{title}</h2>
        {hint !== undefined && <span className="section-hint">{hint}</span>}
        {action !== undefined && <span className="section-action">{action}</span>}
      </header>
      {empty !== undefined ? <p className="section-empty">{empty}</p> : children}
    </section>
  );
}

/** 一排横着放的封面，放不下就横向滚（曲目、专辑、歌手都用它） */
export function Shelf({ children }: { children: ReactNode }) {
  return <div className="shelf">{children}</div>;
}
