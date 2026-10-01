/**
 * 导航图标。内联 SVG，不引图标库（要求书第十五条第 3 点）。
 *
 * 线宽 1.6、圆角端点，照 Windows 的 Segoe Fluent Icons 的观感来，
 * 和播放条里的那几个图标是同一套画法。
 */

const BOX = { viewBox: "0 0 24 24", width: 18, height: 18, fill: "none", stroke: "currentColor", strokeWidth: 1.6, strokeLinecap: "round" as const, strokeLinejoin: "round" as const, "aria-hidden": true };

export type NavIconName =
  | "home" | "kwic" | "library" | "explorer" | "analytics"
  | "import" | "scrape" | "anki" | "dict" | "search" | "menu" | "settings";

export function NavIcon({ name }: { name: NavIconName }) {
  switch (name) {
    case "home":
      return (
        <svg {...BOX}>
          <path d="M4 10.5 12 4l8 6.5" />
          <path d="M6 10v9h12v-9" />
          <path d="M10 19v-5h4v5" />
        </svg>
      );
    case "kwic":
      return (
        <svg {...BOX}>
          <circle cx="11" cy="11" r="6.5" />
          <path d="m16 16 4 4" />
        </svg>
      );
    case "library":
      return (
        <svg {...BOX}>
          <path d="M5 4h4v16H5zM11 4h3v16h-3z" />
          <path d="m16.5 5.5 3.2.9-3.4 13-3.2-.9" />
        </svg>
      );
    case "explorer":
      return (
        <svg {...BOX}>
          <circle cx="9" cy="8.5" r="3.2" />
          <path d="M3.5 19c.6-3 2.8-4.6 5.5-4.6s4.9 1.6 5.5 4.6" />
          <circle cx="17.5" cy="10" r="2.4" />
          <path d="M16 15c2.3.2 3.8 1.5 4.4 4" />
        </svg>
      );
    case "analytics":
      return (
        <svg {...BOX}>
          <path d="M4 19h16" />
          <path d="M7 19V9.5M12 19V5M17 19v-6.5" />
        </svg>
      );
    case "import":
      return (
        <svg {...BOX}>
          <path d="M12 4v10" />
          <path d="m8 10.5 4 4 4-4" />
          <path d="M4.5 17.5V19a1 1 0 0 0 1 1h13a1 1 0 0 0 1-1v-1.5" />
        </svg>
      );
    case "scrape":
      return (
        <svg {...BOX}>
          <circle cx="12" cy="12" r="8" />
          <path d="M4.5 9.5h15M4.5 14.5h15" />
          <path d="M12 4c2.2 2.3 3.3 5 3.3 8s-1.1 5.7-3.3 8c-2.2-2.3-3.3-5-3.3-8S9.8 6.3 12 4z" />
        </svg>
      );
    case "anki":
      return (
        <svg {...BOX}>
          <rect x="4" y="6" width="13" height="12" rx="2" />
          <path d="M8 10h5M8 13.5h3" />
          <path d="M19.5 8.5v9a2 2 0 0 1-2 2h-9" />
        </svg>
      );
    case "dict":
      return (
        <svg {...BOX}>
          <path d="M5 5.5A1.5 1.5 0 0 1 6.5 4H18a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1H6.5A1.5 1.5 0 0 0 5 19.5z" />
          <path d="M5 16.5A1.5 1.5 0 0 1 6.5 15H19" />
          <path d="M9 8.5h6" />
        </svg>
      );
    case "search":
      return (
        <svg {...BOX}>
          <circle cx="11" cy="11" r="6.5" />
          <path d="m16 16 4 4" />
        </svg>
      );
    case "menu":
      return (
        <svg {...BOX}>
          <path d="M4 7h16M4 12h16M4 17h16" />
        </svg>
      );
    case "settings":
      return (
        <svg {...BOX}>
          <circle cx="12" cy="12" r="3.1" />
          <path d="M12 3.6v2.2M12 18.2v2.2M20.4 12h-2.2M5.8 12H3.6M17.9 6.1l-1.6 1.6M7.7 16.3l-1.6 1.6M17.9 17.9l-1.6-1.6M7.7 7.7 6.1 6.1" />
        </svg>
      );
  }
}
