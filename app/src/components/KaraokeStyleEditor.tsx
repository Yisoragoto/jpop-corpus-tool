/**
 * 卡拉 OK 字幕样式的设置：预设、字体字号粗细、没唱 / 唱过两套颜色、描边、发光、立体阴影、两行错开。
 * 设置页「歌词」里一份，卡拉 OK 右上角「字幕样式」面板里一份，改的是同一个存储（`karaokeStyle.ts`）。
 */

import type { CSSProperties } from "react";

import { lyricFontStack, useFontOptions, useLyricFontFiles } from "../fonts";
import { tr } from "../i18n";
import {
  applyPreset,
  KARAOKE_LIMITS,
  karaokeCssVars,
  matchingPreset,
  updateKaraokeStyle,
  useKaraokeStyle,
  type KaraokePresetId,
  type KaraokeStyle,
} from "../karaokeStyle";
import { SettingCard, SettingGroup, SettingSelect, SettingSlider, Switch } from "./SettingCard";

const PRESET_LABELS: Record<KaraokePresetId, string> = {
  classic: tr("经典蓝"),
  pink: tr("粉色像素风"),
  plain: tr("简洁"),
};

const WEIGHTS = [400, 500, 600, 700, 800, 900] as const;

function ColorPick({ value, label, onChange }: { value: string; label: string; onChange: (v: string) => void }) {
  return (
    <label className="ktv-color" title={label}>
      <input type="color" value={value} onChange={(e) => onChange(e.target.value)} aria-label={label} />
      <span>{label}</span>
    </label>
  );
}

/** 预览：一行字，前半唱过、后半没唱，和卡拉 OK 里同一套 CSS */
export function KaraokeStylePreview({ style }: { style: KaraokeStyle }) {
  const vars = karaokeCssVars(style) as CSSProperties;
  const font = style.fontFamily === "" ? undefined : lyricFontStack(style.fontFamily, "");
  const sung = [..."恋のお相手は"];
  const rest = [..."お星様"];
  return (
    <div className="ktv-preview" style={{ ...vars, ...(font ? { ["--ktv-font" as string]: font } : {}) }} lang="ja">
      <div className="ktv-row ktv-row-active">
        <span className="ktv-text">
          {sung.map((ch, i) => <span key={i} className="kc kc-on" data-ch={ch}>{ch}</span>)}
          <span className="kc kc-now" data-ch={rest[0]} style={{ ["--p" as string]: "45%" }}>{rest[0]}</span>
          <ruby>{rest.slice(1).map((ch, i) => <span key={i} className="kc" data-ch={ch}>{ch}</span>)}<rt>ほしさま</rt></ruby>
        </span>
      </div>
    </div>
  );
}

export function KaraokeStyleEditor({ onError, compact = false }: { onError: (message: string) => void; compact?: boolean }) {
  const style = useKaraokeStyle();
  const fonts = useFontOptions(true, onError);
  useLyricFontFiles([style.fontFamily], onError);
  const preset = matchingPreset(style);
  const set = (patch: Partial<KaraokeStyle>) => updateKaraokeStyle(patch);

  const fontOptions = [
    { value: "", label: tr("跟歌词字体一样") },
    ...(fonts.options ?? []).map((o) => ({ value: o.value, label: o.label })),
  ];
  if (style.fontFamily !== "" && !fontOptions.some((o) => o.value === style.fontFamily)) {
    fontOptions.push({ value: style.fontFamily, label: tr("{fontFamily}（本机没有）", { fontFamily: style.fontFamily }) });
  }

  return (
    <div className={`ktv-style-editor ${compact ? "compact" : ""}`}>
      <SettingGroup title={tr("卡拉 OK 字幕")}>
        <SettingCard title={tr("预设")} description={tr("换预设只换颜色、描边、发光和阴影，字体和字号留着")}>
          <span className="segmented ktv-presets" role="radiogroup" aria-label={tr("预设")}>
            {(Object.keys(PRESET_LABELS) as KaraokePresetId[]).map((id) => (
              <button key={id} type="button" role="radio" aria-checked={preset === id}
                className={preset === id ? "on" : ""} onClick={() => updateKaraokeStyle(applyPreset(style, id))}>
                {PRESET_LABELS[id]}
              </button>
            ))}
          </span>
        </SettingCard>
        <SettingCard title={tr("预览")} description={<KaraokeStylePreview style={style} />} />
        <SettingCard
          title={tr("字幕字体")}
          description={fonts.options === null ? tr("正在读取本机字体…") : tr("日文粗体（游ゴシック、Meiryo、BIZ UDPGothic）最像 KTV")}
        >
          <SettingSelect value={style.fontFamily} options={fontOptions} onChange={(v) => set({ fontFamily: v })} />
        </SettingCard>
        <SettingCard title={tr("字幕字号")} description={tr("窗口窄了会按比例缩小")}>
          <SettingSlider value={style.fontSize} limits={KARAOKE_LIMITS.fontSize} onChange={(v) => set({ fontSize: v })} />
        </SettingCard>
        <SettingCard title={tr("粗细")}>
          <SettingSelect
            value={String(style.fontWeight)}
            options={WEIGHTS.map((w) => ({ value: String(w), label: String(w) }))}
            onChange={(v) => set({ fontWeight: Number(v) })}
          />
        </SettingCard>
        <SettingCard title={tr("没唱到的字")} description={tr("填色、描边、发光")}>
          <span className="ktv-colors">
            <ColorPick label={tr("填色")} value={style.unsungFill} onChange={(v) => set({ unsungFill: v })} />
            <ColorPick label={tr("描边")} value={style.unsungStroke} onChange={(v) => set({ unsungStroke: v })} />
            <ColorPick label={tr("发光")} value={style.unsungGlow} onChange={(v) => set({ unsungGlow: v })} />
          </span>
        </SettingCard>
        <SettingCard title={tr("唱过的字")} description={tr("逐字从左往右擦过去")}>
          <span className="ktv-colors">
            <ColorPick label={tr("填色")} value={style.sungFill} onChange={(v) => set({ sungFill: v })} />
            <ColorPick label={tr("描边")} value={style.sungStroke} onChange={(v) => set({ sungStroke: v })} />
            <ColorPick label={tr("发光")} value={style.sungGlow} onChange={(v) => set({ sungGlow: v })} />
          </span>
        </SettingCard>
        <SettingCard title={tr("描边粗细")}>
          <SettingSlider value={style.strokeWidth} limits={KARAOKE_LIMITS.strokeWidth} onChange={(v) => set({ strokeWidth: v })} />
        </SettingCard>
        <SettingCard title={tr("发光强度")} description={tr("0 是不发光")}>
          <SettingSlider value={style.glowStrength} limits={KARAOKE_LIMITS.glowStrength} onChange={(v) => set({ glowStrength: v })} />
        </SettingCard>
        <SettingCard title={tr("立体阴影")} description={tr("往右下方叠出的厚度，让字浮起来；0 是没有")}>
          <span className="ktv-colors">
            <ColorPick label={tr("阴影色")} value={style.shadowColor} onChange={(v) => set({ shadowColor: v })} />
            <SettingSlider value={style.shadowDepth} limits={KARAOKE_LIMITS.shadowDepth} onChange={(v) => set({ shadowDepth: v })} />
          </span>
        </SettingCard>
        <SettingCard title={tr("两行错开")} description={tr("上行靠左、下行靠右；关掉两行都居中")}>
          <Switch checked={style.staggered} onChange={(v) => set({ staggered: v })} label={tr("两行错开")} />
        </SettingCard>
      </SettingGroup>
    </div>
  );
}
