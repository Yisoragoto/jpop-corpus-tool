/**
 * 歌词区右上角：振假名开关、「显示」面板（字体、字号、振假名字号、行距、字距、注音方式），可选的「全屏」按钮。
 * 改了立刻生效、自动记住，没有「确定」按钮；点面板外面或按 Esc 收起。
 */

import { useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { lyricFontStack, useFontOptions, type FontOption } from "../fonts";
import {
  DISPLAY_LIMITS,
  FURIGANA_MODE_LABELS,
  resetLyricsDisplay,
  updateLyricsDisplay,
  type FuriganaMode,
  type LyricsDisplay,
} from "../lyricsDisplay";

interface Props {
  display: LyricsDisplay;
  onError: (message: string) => void;
  /** 给了就显示「全屏」按钮 */
  onExpand?: () => void;
}

/** 面板里的预览句（和 Python 版一样） */
const PREVIEW_TEXT = "忘れたい自分に缶コーヒーを買った";

export function LyricsTools({ display, onError, onExpand }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const fonts = useFontOptions(open, onError);
  // 刚导入的文件：字体列表刷新后把它设成主字体
  const [justImported, setJustImported] = useState<string[]>([]);

  useEffect(() => {
    if (!open) return;
    const onPointer = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => {
    if (justImported.length === 0 || fonts.options === null) return;
    const imported = fonts.options.find((o) => o.file !== undefined && justImported.includes(o.file.path));
    if (imported !== undefined) {
      updateLyricsDisplay({ fontFamily: imported.value });
      setJustImported([]);
    } else if (fonts.problems.some((p) => justImported.some((path) => p.includes(path)))) {
      setJustImported([]);
    }
  }, [justImported, fonts.options, fonts.problems]);

  const importFonts = async () => {
    try {
      const picked = await openDialog({
        multiple: true,
        title: "导入字体",
        filters: [{ name: "字体文件", extensions: ["ttf", "otf", "ttc"] }],
      });
      const paths = picked === null ? [] : Array.isArray(picked) ? picked : [picked];
      if (paths.length === 0) return;
      setJustImported(paths);
      fonts.addImported(paths);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  };

  // 导入过、现在读不到的文件
  const brokenImports = fonts.imported.filter((path) => fonts.problems.some((p) => p.includes(path)));

  return (
    <div className="lyrics-tools" ref={ref}>
      <button
        className={`tool-toggle ${display.furigana ? "on" : ""}`}
        onClick={() => updateLyricsDisplay({ furigana: !display.furigana })}
        aria-pressed={display.furigana}
        title={display.furigana ? "隐藏振假名" : "显示振假名"}
      >
        <ruby>
          振<rt>ふり</rt>
        </ruby>
        假名
      </button>
      <button
        className={`tool-toggle ${open ? "on" : ""}`}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        title="歌词字体、字号、振假名、行距、字距"
      >
        显示
      </button>
      {onExpand && (
        <button className="tool-toggle" onClick={onExpand} title="全屏歌词（Esc 退出）">
          全屏
        </button>
      )}
      {open && (
        <div className="lyrics-display-panel" role="dialog" aria-label="歌词显示">
          <FontSelect
            label="主字体"
            value={display.fontFamily}
            emptyLabel="默认（界面字体）"
            options={fonts.options}
            onChange={(v) => updateLyricsDisplay({ fontFamily: v })}
          />
          <FontSelect
            label="备用字体"
            value={display.fallbackFamily}
            emptyLabel="不设"
            options={fonts.options}
            onChange={(v) => updateLyricsDisplay({ fallbackFamily: v })}
            title="主字体里没有的字用这个显示"
          />
          <div className="display-row">
            <span className="display-label" />
            <button className="link-btn" onClick={() => void importFonts()} title="ttf / otf / ttc。只记住文件位置，不复制">
              导入字体…
            </button>
            {brokenImports.length > 0 && (
              <button
                className="link-btn warn"
                onClick={() => brokenImports.forEach(fonts.removeImported)}
                title={brokenImports.join("\n")}
              >
                清除 {brokenImports.length} 个读不到的
              </button>
            )}
          </div>
          <p
            className="font-preview"
            lang="ja"
            style={{
              fontFamily: lyricFontStack(display.fontFamily, display.fallbackFamily),
              fontSize: display.fontSize,
              letterSpacing: display.letterSpacing,
            }}
          >
            {PREVIEW_TEXT}
          </p>
          <Slider label="字号" value={display.fontSize} limits={DISPLAY_LIMITS.fontSize} onChange={(v) => updateLyricsDisplay({ fontSize: v })} />
          <Slider
            label="振假名字号"
            value={display.rubySize}
            limits={DISPLAY_LIMITS.rubySize}
            onChange={(v) => updateLyricsDisplay({ rubySize: v })}
          />
          <Slider label="行距" value={display.lineGap} limits={DISPLAY_LIMITS.lineGap} onChange={(v) => updateLyricsDisplay({ lineGap: v })} />
          <Slider
            label="字距"
            value={display.letterSpacing}
            limits={DISPLAY_LIMITS.letterSpacing}
            onChange={(v) => updateLyricsDisplay({ letterSpacing: v })}
          />
          <div className="display-row">
            <span className="display-label">注音方式</span>
            <div className="segmented">
              {(Object.keys(FURIGANA_MODE_LABELS) as FuriganaMode[]).map((mode) => (
                <button
                  key={mode}
                  className={display.furiganaMode === mode ? "on" : ""}
                  onClick={() => updateLyricsDisplay({ furiganaMode: mode, furigana: true })}
                  title={mode === "kanji" ? "例：思(おも)い出(だ)す" : "例：思い出す(おもいだす)"}
                >
                  {FURIGANA_MODE_LABELS[mode]}
                </button>
              ))}
            </div>
          </div>
          <div className="display-foot">
            <button className="link-btn" onClick={resetLyricsDisplay}>
              恢复默认
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function FontSelect({
  label,
  value,
  emptyLabel,
  options,
  onChange,
  title,
}: {
  label: string;
  value: string;
  emptyLabel: string;
  options: FontOption[] | null;
  onChange: (value: string) => void;
  title?: string;
}) {
  const known = options === null || value === "" || options.some((o) => o.value === value);
  return (
    <label className="display-row" title={title}>
      <span className="display-label">{label}</span>
      <select className="font-select" value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">{emptyLabel}</option>
        {options === null && value !== "" && <option value={value}>{value}</option>}
        {!known && <option value={value}>{value}（本机没有）</option>}
        {options?.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
        {options === null && <option disabled>读取字体…</option>}
      </select>
    </label>
  );
}

function Slider({
  label,
  value,
  limits: [min, max],
  onChange,
}: {
  label: string;
  value: number;
  limits: readonly [number, number];
  onChange: (value: number) => void;
}) {
  return (
    <label className="display-row">
      <span className="display-label">{label}</span>
      <input type="range" min={min} max={max} step={1} value={value} onChange={(e) => onChange(Number(e.target.value))} />
      <span className="display-value">{value}px</span>
    </label>
  );
}
