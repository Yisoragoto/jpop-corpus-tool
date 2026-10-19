import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./ErrorBoundary";
import "./styles.css";
import { language } from "./i18n";

document.documentElement.lang = language() === "zh" ? "zh-CN" : "en";
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
