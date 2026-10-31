import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./ErrorBoundary";
import "./styles.css";
import { language } from "./i18n";
import { getAppSettings } from "./settings";
import { resolveTheme } from "./theme";

document.documentElement.lang = language() === "zh" ? "zh-CN" : "en";
document.documentElement.dataset.theme = resolveTheme(getAppSettings().theme, window.matchMedia("(prefers-color-scheme: dark)").matches);
import { tr } from "./i18n";

const root = document.getElementById("root");
if (!root) throw new Error(tr("找不到 #root"));

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
