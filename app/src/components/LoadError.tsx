/**
 * 页面首次读取失败时的那一块：说清楚没读出来、为什么，给一个重试。
 * 以前失败只弹一条全局通知，页面停在「加载中…」，看上去像卡住了。
 */

import { tr } from "../i18n";

interface Props {
  message: string;
  onRetry: () => void;
}

export function LoadError({ message, onRetry }: Props) {
  return (
    <div className="page pad load-error" role="alert">
      <h2>{tr("没有读出来")}</h2>
      <p className="muted">{message}</p>
      <button className="btn" onClick={onRetry}>{tr("重试")}</button>
    </div>
  );
}
