import { Icon, type IconName } from "./Icon";

export type NavIconName = "home" | "kwic" | "library" | "analytics" | "import" | "scrape" | "anki" | "dict" | "search" | "menu" | "settings";
const names: Record<NavIconName, IconName> = { home: "home", kwic: "search", library: "library", analytics: "analytics", import: "import", scrape: "globe", anki: "card", dict: "dict", search: "search", menu: "menu", settings: "settings" };
export function NavIcon({ name }: { name: NavIconName }) { return <Icon name={names[name]} size={20} />; }
