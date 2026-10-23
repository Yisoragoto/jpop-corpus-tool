/**
 * 第一次启动时选界面语言。两种语言的字都同时写着，不管默认落在哪种语言都看得懂。
 *
 * 默认选中系统语言（系统是中文就中文，否则英文）。选了之后记下来，以后不再弹；
 * 选的和现在显示的不一样就整页重新加载一次。设置 → 外观 → 界面语言随时可以改。
 */

import { useEffect, useState } from "react";

import { language, LANGUAGES, setLanguage, systemLanguage, type Lang } from "../i18n";

const HINT: Record<Lang, string> = {
  zh: "以后可以在 设置 → 外观 里改",
  en: "You can change this later in Settings → Appearance",
};

export function LanguagePicker({ onDone }: { onDone: () => void }) {
  const [choice, setChoice] = useState<Lang>(() => systemLanguage());

  const confirm = (lang: Lang) => {
    const reload = lang !== language();
    setLanguage(lang, reload);
    if (!reload) onDone();
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter") confirm(choice);
      if (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "ArrowUp" || e.key === "ArrowDown") {
        setChoice((c) => (c === "zh" ? "en" : "zh"));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // confirm 每次渲染都是新的，只依赖 choice 就够了
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [choice]);

  return (
    <div className="palette-backdrop update-backdrop lang-backdrop">
      <div className="update-dialog lang-dialog" role="dialog" aria-modal="true" aria-label="Language / 界面语言">
        <h2>
          <span lang="en">Choose your language</span>
          <span lang="zh-CN">选择界面语言</span>
        </h2>
        <div className="lang-options" role="radiogroup">
          {LANGUAGES.map((l) => (
            <button
              key={l.id}
              role="radio"
              aria-checked={choice === l.id}
              className={`lang-option${choice === l.id ? " on" : ""}`}
              onClick={() => setChoice(l.id)}
              onDoubleClick={() => confirm(l.id)}
            >
              <strong>{l.label}</strong>
              <small>{l.id === "zh" ? "简体中文" : "English"}</small>
            </button>
          ))}
        </div>
        <p className="lang-hint">{HINT[choice]}</p>
        <button className="btn primary lang-go" onClick={() => confirm(choice)} autoFocus>
          {choice === "zh" ? "开始使用" : "Continue"}
        </button>
      </div>
    </div>
  );
}
