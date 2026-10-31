import { Icon, type IconName } from "./Icon";
/**
 * 命令栏按钮：图标 + 文字，照 Windows 应用顶部那排命令。
 *
 * 各页的主要操作都用它，样子才统一；次要操作仍然用 `.link-btn` 这种纯文字的。
 * 图标是内联 SVG，不引图标库。
 */

import type { ReactNode } from "react";

export type CommandIcon =
  | "folder" | "scan" | "import" | "cancel" | "discard" | "scrape" | "retry"
  | "photo" | "album" | "export" | "refresh" | "report" | "add" | "clear" | "check";

interface Props {
  icon: CommandIcon;
  label: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  title?: string;
  /** 这一排里最主要的那个 */
  primary?: boolean;
}

export function CommandButton({ icon, label, onClick, disabled = false, title, primary = false }: Props) {
  return (
    <button className={`cmd-btn ${primary ? "primary" : ""}`} onClick={onClick} disabled={disabled} title={title}>
      <Icon name={commandIcons[icon]} />
      <span>{label}</span>
    </button>
  );
}

const commandIcons: Record<CommandIcon, IconName> = { folder: "folder", scan: "search", import: "import", cancel: "close", discard: "delete", scrape: "globe", retry: "refresh", photo: "photo", album: "album", export: "export", refresh: "refresh", report: "report", add: "add", clear: "close", check: "check" };
