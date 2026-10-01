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
      <Icon name={icon} />
      <span>{label}</span>
    </button>
  );
}

const STROKE = {
  viewBox: "0 0 24 24",
  width: 16,
  height: 16,
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.7,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

function Icon({ name }: { name: CommandIcon }) {
  switch (name) {
    case "folder":
      return (
        <svg {...STROKE}>
          <path d="M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2.5h8a1.5 1.5 0 0 1 1.5 1.5v7A1.5 1.5 0 0 1 19 18.5H5A1.5 1.5 0 0 1 3.5 17z" />
        </svg>
      );
    case "scan":
      return (
        <svg {...STROKE}>
          <circle cx="11" cy="11" r="6" />
          <path d="m15.5 15.5 4 4" />
          <path d="M9 11h4" />
        </svg>
      );
    case "import":
      return (
        <svg {...STROKE}>
          <path d="M12 4v10" />
          <path d="m8 10.5 4 4 4-4" />
          <path d="M4.5 17.5V19a1 1 0 0 0 1 1h13a1 1 0 0 0 1-1v-1.5" />
        </svg>
      );
    case "cancel":
      return (
        <svg {...STROKE}>
          <circle cx="12" cy="12" r="8" />
          <path d="m9 9 6 6M15 9l-6 6" />
        </svg>
      );
    case "discard":
      return (
        <svg {...STROKE}>
          <path d="M5 7h14" />
          <path d="M9.5 7V5.5h5V7" />
          <path d="M6.5 7l.8 11a1.5 1.5 0 0 0 1.5 1.4h6.4a1.5 1.5 0 0 0 1.5-1.4L17.5 7" />
        </svg>
      );
    case "scrape":
      return (
        <svg {...STROKE}>
          <circle cx="12" cy="12" r="8" />
          <path d="M4.5 9.5h15M4.5 14.5h15" />
          <path d="M12 4c2.2 2.3 3.3 5 3.3 8s-1.1 5.7-3.3 8c-2.2-2.3-3.3-5-3.3-8S9.8 6.3 12 4z" />
        </svg>
      );
    case "retry":
      return (
        <svg {...STROKE}>
          <path d="M20 12a8 8 0 1 1-2.6-5.9" />
          <path d="M20 4v5h-5" />
        </svg>
      );
    case "photo":
      return (
        <svg {...STROKE}>
          <rect x="4" y="5.5" width="16" height="13" rx="2" />
          <circle cx="9" cy="10" r="1.6" />
          <path d="m5.5 17 4.2-4.2 3 3 2.3-2.3 3.5 3.5" />
        </svg>
      );
    case "album":
      return (
        <svg {...STROKE}>
          <circle cx="12" cy="12" r="8" />
          <circle cx="12" cy="12" r="2.2" />
        </svg>
      );
    case "export":
      return (
        <svg {...STROKE}>
          <path d="M12 16V5" />
          <path d="m8 8.5 4-4 4 4" />
          <path d="M5 15v3.5a1.5 1.5 0 0 0 1.5 1.5h11a1.5 1.5 0 0 0 1.5-1.5V15" />
        </svg>
      );
    case "refresh":
      return (
        <svg {...STROKE}>
          <path d="M4 12a8 8 0 0 1 13.7-5.6L20 8.5" />
          <path d="M20 4v4.5h-4.5" />
          <path d="M20 12a8 8 0 0 1-13.7 5.6L4 15.5" />
          <path d="M4 20v-4.5h4.5" />
        </svg>
      );
    case "report":
      return (
        <svg {...STROKE}>
          <rect x="4.5" y="4" width="15" height="16" rx="2" />
          <path d="M8.5 15v-3M12 15V9M15.5 15v-5" />
        </svg>
      );
    case "add":
      return (
        <svg {...STROKE}>
          <path d="M12 5v14M5 12h14" />
        </svg>
      );
    case "clear":
      return (
        <svg {...STROKE}>
          <rect x="4.5" y="4.5" width="15" height="15" rx="3" />
          <path d="m9 9 6 6M15 9l-6 6" />
        </svg>
      );
    case "check":
      return (
        <svg {...STROKE}>
          <path d="m5 12.5 4.5 4.5L19 7.5" />
        </svg>
      );
  }
}
