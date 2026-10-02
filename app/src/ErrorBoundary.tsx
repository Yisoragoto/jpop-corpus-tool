/**
 * 兜住渲染期的异常。
 *
 * **为什么需要**：React 里任何一个组件渲染时抛错，默认行为是把整棵树卸载掉。
 * 在浏览器里那是一个空白标签页，在这个应用里是一整扇白窗口——没有报错、
 * 没有按钮、没有菜单，用户唯一能做的事是把它关掉重开，而我这边什么线索都拿不到。
 *
 * 这里不做「自动恢复」那一套（重试、回滚状态、上报）。只做三件事：
 * 说出错了、说是什么错、给一条能继续走的路（重载界面）。
 * 真正的线索在日志和「导出诊断信息」里，这段文案把人指过去。
 */

import { Component, type ErrorInfo, type ReactNode } from "react";

import { crashNotice } from "./errors";

interface Props {
  children: ReactNode;
}

interface State {
  error: unknown;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: unknown): State {
    return { error };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    // 控制台留一份完整的：组件栈只在这儿有，界面上放不下
    console.error("界面渲染出错", error, info.componentStack);
  }

  render() {
    if (this.state.error === null) return this.props.children;
    const { title, detail } = crashNotice(this.state.error);
    return (
      <div className="crash">
        <h1>{title}</h1>
        <p className="crash-detail">{detail}</p>
        <p className="muted small">
          数据没有受影响——界面是只读地显示库里的东西。
          重载之后如果还是这样，到「设置 → 系统 → 导出诊断信息」把那段文字发给我。
        </p>
        <button type="button" onClick={() => window.location.reload()}>
          重载界面
        </button>
      </div>
    );
  }
}
