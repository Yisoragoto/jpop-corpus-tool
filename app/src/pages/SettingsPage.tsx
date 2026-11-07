import { Icon } from "../components/Icon";
/**
 * 设置页。照 Windows 11 设置 / WinUI 的 SettingsCard：一组一组的行，
 * 左边图标、中间标题和说明、右边控件。**改了立刻生效、自动记住**，没有「确定」。
 *
 * 这里不新造一份状态：每一行读写的都是原来那处的存储——
 * 外观和曲库视图在 `settings.ts`，歌词字体字号在 `lyricsDisplay.ts`，
 * 制卡在 `dict/mine.ts`。所以在歌词右上角的「显示」面板里改，这一页跟着变，反过来也一样。
 */

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import {
  api,
  type AnkiStatus,
  type HealthReport,
  type LibraryRootInfo,
} from "../api";
import { CommandButton } from "../components/CommandButton";
import { SettingCard, SettingGroup, SettingNote, SettingSelect, SettingSlider, Switch } from "../components/SettingCard";
import { setDictionariesCollapsed, useCollapsedDictionaries } from "../dict/collapse";
import { DictionaryManager } from "../dict/DictionaryManager";
import { BuildInfo, UpdateCard } from "../components/UpdateCard";
import { MigrateCard } from "../components/MigrateCard";
import { SudachiCard } from "../components/SudachiCard";
import { TokenizeCard } from "../components/TokenizeCard";
import { PitchCacheCard } from "../components/PitchCacheCard";
import { DiagnosticsCard } from "../components/DiagnosticsCard";
import { MineSettingsCard } from "../dict/MineSettingsCard";
import { KaraokeStyleEditor } from "../components/KaraokeStyleEditor";
import { lyricFontStack, pickFontFiles, useFontOptions, useLyricFontFiles } from "../fonts";
import {
  DISPLAY_LIMITS,
  FURIGANA_MODE_LABELS,
  resetLyricsDisplay,
  updateLyricsDisplay,
  useLyricsDisplay,
  type FuriganaMode,
} from "../lyricsDisplay";
import { DAILY_GOAL_LIMITS, DEFAULT_SETTINGS, STAGE_BLUR_LIMITS, UI_SCALE_LIMITS, UI_SCALE_STEP, updateSettings, useAppSettings } from "../settings";
import { language, LANGUAGES, setLanguage, tr, type Lang } from "../i18n";

interface Props {
  health: HealthReport | null;
  onError: (message: string) => void;
  /** 库被改动了（补分词、换分词词典、迁移），让外壳重新拉总览 */
  onChanged: () => void;
}

const Icons = {
  globe: <Icon name="globe" />,
  report: <Icon name="report" />,
  stage: <Icon name="stage" />,
  blur: <Icon name="blur" />,
  grid: <Icon name="grid" />,
  font: <Icon name="font" />,
  ruby: <Icon name="ruby" />,
  size: <Icon name="size" />,
  dict: <Icon name="dict" />,
  card: <Icon name="card" />,
  db: <Icon name="db" />,
  cover: <Icon name="album" />,
  clock: <Icon name="clock" />,
  update: <Icon name="refresh" />,
  install: <Icon name="import" />,
  wave: <Icon name="wave" />,
  lyrics: <Icon name="lyrics" />,
  info: <Icon name="info" />,
};

/** 语料库是怎么找到的，如实说 */
const ROOT_SOURCE_LABELS: Record<LibraryRootInfo["source"], string> = {
  env: tr("来自 JPOP_CORPUS_HOME"),
  settings: tr("在设置里选的"),
  nextToExe: tr("在程序旁边找到的"),
  workingDir: tr("当前工作目录"),
  default: tr("默认数据目录"),
};

type SectionId = "appearance" | "lyrics" | "wordcloud" | "dict" | "mining" | "corpus" | "system" | "about";

/** 左边那一列：点一项换一页，不再是一整条长页面里跳锚点 */
const SECTIONS: { id: SectionId; label: string; lead: string; icon: keyof typeof Icons }[] = [
  { id: "appearance", label: tr("外观"), lead: tr("语言、背景、播放条、收听目标"), icon: "stage" },
  { id: "lyrics", label: tr("歌词与播放"), lead: tr("振假名、字体、卡拉 OK 字幕"), icon: "lyrics" },
  { id: "wordcloud", label: tr("词云"), lead: tr("词云的字体和不显示的词"), icon: "font" },
  { id: "dict", label: tr("查词"), lead: tr("查词词典的导入、启用和顺序"), icon: "dict" },
  { id: "mining", label: tr("制卡"), lead: tr("Anki 牌组、笔记类型和字段"), icon: "card" },
  { id: "corpus", label: tr("语料处理"), lead: tr("分词词典、补分词、迁移"), icon: "db" },
  { id: "system", label: tr("更新与系统"), lead: tr("更新、诊断、变调缓存"), icon: "update" },
  { id: "about", label: tr("关于"), lead: tr("语料库目录和运行状态"), icon: "info" },
];

/** 上次看的是哪一页：跟着模块走，切到别的页再回来还在 */
let lastSection: SectionId = "appearance";

/** 从别处直接打开设置的某一页（例：词典页的「去设置里管理」） */
export function openSettingsSection(id: SectionId) {
  lastSection = id;
}

export function SettingsPage({ health, onError, onChanged }: Props) {
  const settings = useAppSettings();
  const display = useLyricsDisplay();
  const fonts = useFontOptions(true, onError);
  // 预览要真用上选的字体：导入的、自带的字体是按文件加载的
  useLyricFontFiles([settings.wordCloudFont, display.fontFamily, display.fallbackFamily, settings.uiFont], onError);
  const [anki, setAnki] = useState<AnkiStatus | null>(null);
  const [root, setRoot] = useState<LibraryRootInfo | null>(null);
  const [rootNote, setRootNote] = useState("");
  const collapsedDicts = useCollapsedDictionaries();
  const [section, setSection] = useState<SectionId>(lastSection);
  const bodyRef = useRef<HTMLDivElement | null>(null);

  const goTo = useCallback((id: SectionId) => {
    lastSection = id;
    setSection(id);
    // 换页从顶上开始看
    bodyRef.current?.closest(".page")?.scrollTo({ top: 0 });
  }, []);

  useEffect(() => {
    void api.ankiStatus().then(setAnki).catch(() => undefined);
    void api.libraryRoot().then(setRoot).catch(() => undefined);
  }, []);

  /** 换语料库目录。**只写设置**，要重启才生效——连接和放行范围都是启动时定下的 */
  const pickLibraryRoot = useCallback(async () => {
    try {
      const picked = await openDialog({ directory: true, title: tr("选择语料库目录（含 corpus.db）") });
      if (typeof picked !== "string") return;
      setRootNote(await api.setLibraryRoot(picked));
      setRoot(await api.libraryRoot());
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [onError]);

  const importFonts = useCallback(async () => {
    try {
      const paths = await pickFontFiles();
      if (paths.length > 0) fonts.addImported(paths);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [fonts, onError]);

  const fontOptions = [
    { value: "", label: tr("默认（界面字体）") },
    ...(fonts.options ?? []).map((o) => ({ value: o.value, label: o.label })),
  ];
  // 选中的字体本机没有时也要列出来，否则下拉显示成空白
  if (display.fontFamily !== "" && !fontOptions.some((o) => o.value === display.fontFamily)) {
    fontOptions.push({ value: display.fontFamily, label: tr("{fontFamily}（本机没有）", { fontFamily: display.fontFamily }) });
  }
  const cloudFontOptions = [...fontOptions];
  if (settings.wordCloudFont !== "" && !cloudFontOptions.some((o) => o.value === settings.wordCloudFont)) {
    cloudFontOptions.push({ value: settings.wordCloudFont, label: tr("{wordCloudFont}（本机没有）", { wordCloudFont: settings.wordCloudFont }) });
  }
  const uiFontOptions = [{ value: "", label: tr("系统默认") }, ...fontOptions.slice(1)];
  if (settings.uiFont !== "" && !uiFontOptions.some((o) => o.value === settings.uiFont)) {
    uiFontOptions.push({ value: settings.uiFont, label: tr("{uiFont}（本机没有）", { uiFont: settings.uiFont }) });
  }
  const fallbackOptions = [{ value: "", label: tr("不设") }, ...fontOptions.slice(1)];
  if (display.fallbackFamily !== "" && !fallbackOptions.some((o) => o.value === display.fallbackFamily)) {
    fallbackOptions.push({ value: display.fallbackFamily, label: tr("{fallbackFamily}（本机没有）", { fallbackFamily: display.fallbackFamily }) });
  }

  const current = SECTIONS.find((x) => x.id === section) ?? SECTIONS[0]!;

  return (
    <div className="page settings-page">
      <div className="settings-layout">
        <nav className="settings-nav" aria-label={tr("设置分类")}>
          {SECTIONS.map((x) => (
            <button
              key={x.id}
              type="button"
              className={x.id === section ? "settings-nav-item active" : "settings-nav-item"}
              onClick={() => goTo(x.id)}
              aria-current={x.id === section ? "page" : undefined}
            >
              <span className="settings-nav-icon">{Icons[x.icon]}</span>
              {x.label}
            </button>
          ))}
        </nav>
        <div className="settings-body" key={section} ref={bodyRef}>
          <header className="settings-head">
            <span className="settings-head-icon">{Icons[current.icon]}</span>
            <div>
              <h1>{current.label}</h1>
              <p>{current.lead}</p>
            </div>
          </header>

          {section === "appearance" && (
            <Group title={tr("主题")}>
              <SettingCard
                icon={<Icon name="theme" />}
                title={tr("浅色 / 深色")}
                description={tr("全屏歌词和卡拉 OK 始终是深色")}
              >
                <SettingSelect value={settings.theme} options={[
                  { value: "system", label: tr("跟随系统") },
                  { value: "light", label: tr("浅色") },
                  { value: "dark", label: tr("深色") },
                ]} onChange={(theme) => updateSettings({ theme })} />
              </SettingCard>
            </Group>
          )}
          {section === "appearance" && (
            <Group title={tr("语言")}>
              <SettingCard
                icon={Icons.globe}
                title={language() === "zh" ? "界面语言 · Language" : "Language"}
                description={tr("切换时界面会重新加载")}
              >
                <SettingSelect<Lang>
                  value={language()}
                  options={LANGUAGES.map((l) => ({ value: l.id, label: l.label }))}
                  onChange={(v) => setLanguage(v)}
                />
              </SettingCard>
            </Group>
          )}
          {section === "appearance" && (
            <Group title={tr("全屏与播放")}>
            <SettingCard
              icon={Icons.stage}
              title={tr("全屏歌词的背景用封面虚化")}
              description={tr("关掉就用封面的主色")}
            >
              <Switch
                checked={settings.stageCoverBlur}
                onChange={(v) => updateSettings({ stageCoverBlur: v })}
                label={tr("全屏背景用封面虚化")}
              />
            </SettingCard>
            {settings.stageCoverBlur && (
              <SettingCard icon={Icons.blur} title={tr("虚化强度")}>
                <SettingSlider
                  value={settings.stageBlurRadius}
                  limits={STAGE_BLUR_LIMITS}
                  onChange={(v) => updateSettings({ stageBlurRadius: v })}
                />
              </SettingCard>
            )}
            <SettingCard
              icon={Icons.wave}
              title={tr("播放条上的音频可视化")}
              description={tr("关掉后不再采样")}
            >
              <Switch
                checked={settings.spectrum}
                onChange={(v) => updateSettings({ spectrum: v })}
                label={tr("显示音频可视化")}
              />
            </SettingCard>
            <SettingCard icon={Icons.clock} title={tr("每天的收听目标")} description={tr("首页「今日」那个环按这个算")}>
              <SettingSlider
                value={settings.dailyGoalMinutes}
                limits={DAILY_GOAL_LIMITS}
                onChange={(v) => updateSettings({ dailyGoalMinutes: v })}
                unit={tr(" 分钟")}
              />
            </SettingCard>
            </Group>
          )}

          {section === "lyrics" && (
            <>
              <Group title={tr("振假名")}>
            <SettingCard icon={Icons.ruby} title={tr("默认显示振假名")} description={tr("也可以在歌词右上角随时开关")}>
              <Switch checked={display.furigana} onChange={(v) => updateLyricsDisplay({ furigana: v })} label={tr("显示振假名")} />
            </SettingCard>
            <SettingCard icon={Icons.ruby} title={tr("注音方式")} description={tr("只注汉字：思(おも)い出(だ)す；整词注音：思い出す(おもいだす)")}>
              <SettingSelect<FuriganaMode>
                value={display.furiganaMode}
                options={(Object.keys(FURIGANA_MODE_LABELS) as FuriganaMode[]).map((mode) => ({
                  value: mode,
                  label: FURIGANA_MODE_LABELS[mode],
                }))}
                onChange={(mode) => updateLyricsDisplay({ furiganaMode: mode, furigana: true })}
              />
            </SettingCard>
              </Group>
              <Group title={tr("播放")}>
            <SettingCard
              icon={Icons.lyrics}
              title={tr("逐字点亮当前行")}
              description={tr("没有逐字时间的行整行高亮")}
            >
              <Switch
                checked={settings.karaokeLyrics}
                onChange={(v) => updateSettings({ karaokeLyrics: v })}
                label={tr("逐字点亮当前行")}
              />
            </SettingCard>
              </Group>
              <Group title={tr("字体与排版")}>
            <SettingCard icon={Icons.font} title={tr("界面大小")} description={tr("也可以按 Ctrl + = / - / 0")}>
              <SettingSlider value={settings.uiScale} limits={UI_SCALE_LIMITS} step={UI_SCALE_STEP} unit="%" onChange={(uiScale) => updateSettings({ uiScale })} />
            </SettingCard>
            <SettingCard icon={Icons.font} title={tr("界面字体")}>
              <SettingSelect value={settings.uiFont} options={uiFontOptions} onChange={(uiFont) => updateSettings({ uiFont })} />
            </SettingCard>
            <SettingCard
              icon={Icons.font}
              title={tr("歌词字体")}
              description={fonts.options === null ? tr("正在读取本机字体…") : tr("本机 {length} 种", { length: fonts.options.length })}
            >
              <SettingSelect value={display.fontFamily} options={fontOptions} onChange={(v) => updateLyricsDisplay({ fontFamily: v })} />
            </SettingCard>
            <SettingCard icon={Icons.font} title={tr("备用字体")} description={tr("主字体里没有的字用这个显示")}>
              <SettingSelect
                value={display.fallbackFamily}
                options={fallbackOptions}
                onChange={(v) => updateLyricsDisplay({ fallbackFamily: v })}
              />
            </SettingCard>
            <SettingCard icon={Icons.font} title={tr("导入字体")} description={tr("ttf / otf / ttc，或字体 zip")}>
              <CommandButton icon="import" label={tr("导入字体…")} onClick={() => void importFonts()} />
            </SettingCard>
            <SettingCard icon={Icons.size} title={tr("字号")}>
              <SettingSlider value={display.fontSize} limits={DISPLAY_LIMITS.fontSize} onChange={(v) => updateLyricsDisplay({ fontSize: v })} />
            </SettingCard>
            <SettingCard icon={Icons.size} title={tr("振假名字号")}>
              <SettingSlider value={display.rubySize} limits={DISPLAY_LIMITS.rubySize} onChange={(v) => updateLyricsDisplay({ rubySize: v })} />
            </SettingCard>
            <SettingCard icon={Icons.size} title={tr("行距")}>
              <SettingSlider value={display.lineGap} limits={DISPLAY_LIMITS.lineGap} onChange={(v) => updateLyricsDisplay({ lineGap: v })} />
            </SettingCard>
            <SettingCard icon={Icons.size} title={tr("字距")}>
              <SettingSlider
                value={display.letterSpacing}
                limits={DISPLAY_LIMITS.letterSpacing}
                onChange={(v) => updateLyricsDisplay({ letterSpacing: v })}
              />
            </SettingCard>
            <SettingCard
              title={tr("预览")}
              description={
                <span
                  className="setting-preview"
                  lang="ja"
                  style={{
                    fontFamily: lyricFontStack(display.fontFamily, display.fallbackFamily),
                    fontSize: display.fontSize,
                    letterSpacing: display.letterSpacing,
                  }}
                >
                  忘れたい自分に缶コーヒーを買った
                </span>
              }
            >
              <button className="link-btn" onClick={resetLyricsDisplay}>{tr("恢复默认")}</button>
            </SettingCard>
              </Group>
              <KaraokeStyleEditor onError={onError} />
            </>
          )}

          {section === "wordcloud" && (
            <>
            <Group title={tr("哪些词进词云")}>
              <SettingCard
                icon={Icons.font}
                title={tr("去掉虚词")}
                description={tr("假名碎片、形式名词、符号")}
              >
                <Switch
                  checked={settings.wordCloudHideFunctionWords}
                  onChange={(v) => updateSettings({ wordCloudHideFunctionWords: v })}
                  label={tr("词云去掉虚词")}
                />
              </SettingCard>
              <SettingCard
                icon={Icons.font}
                title={tr("自己隐藏的词")}
                description={
                  settings.wordCloudHidden.length === 0 ? (
                    tr("在词云里点词可以隐藏它")
                  ) : (
                    <span className="hidden-words">
                      {settings.wordCloudHidden.map((w) => (
                        <button
                          key={w}
                          className="hidden-word"
                          lang="ja"
                          title={tr("放回词云")}
                          onClick={() => updateSettings({ wordCloudHidden: settings.wordCloudHidden.filter((x) => x !== w) })}
                        >
                          {w}
                          <span aria-hidden="true"><Icon name="close" /></span>
                        </button>
                      ))}
                    </span>
                  )
                }
              >
                {settings.wordCloudHidden.length > 0 && (
                  <button className="link-btn" onClick={() => updateSettings({ wordCloudHidden: [] })}>{tr("全部恢复")}</button>
                )}
              </SettingCard>
            </Group>
            <Group title={tr("字体")}>
            <SettingCard
              icon={Icons.font}
              title={tr("词云字体")}
            >
              <SettingSelect
                value={settings.wordCloudFont}
                options={cloudFontOptions}
                onChange={(v) => updateSettings({ wordCloudFont: v })}
              />
            </SettingCard>
            <SettingNote>
              <button className="link-btn" onClick={() => goTo("lyrics")}>{tr("导入字体在「歌词与播放」里")}</button>
            </SettingNote>
            <SettingCard
              title={tr("预览")}
              description={
                <span className="setting-preview" lang="ja" style={{ fontFamily: lyricFontStack(settings.wordCloudFont, ""), fontSize: 26 }}>
                  君 僕 夢 世界 抱きしめる
                </span>
              }
            >
              <button className="link-btn" onClick={() => updateSettings({ wordCloudFont: DEFAULT_SETTINGS.wordCloudFont })}>{tr("恢复默认")}</button>
            </SettingCard>
            </Group>
            </>
          )}

          {section === "dict" && (
            <>
              <DictionaryManager onError={onError} />
              <Group title={tr("查词结果")}>
            <SettingCard icon={Icons.dict} title={tr("折叠记忆")} description={tr("点词典名折叠，之后一直保持")}>
              <button
                className="tc-btn"
                disabled={collapsedDicts.size === 0}
                onClick={() => setDictionariesCollapsed([...collapsedDicts], false)}
              >
                {collapsedDicts.size === 0 ? tr("没有折叠的") : tr("展开这 {size} 本", { size: collapsedDicts.size })}
              </button>
            </SettingCard>
              </Group>
            </>
          )}

          {section === "mining" && (
            <Group title="Anki">
            <MineSettingsCard connected={anki?.connected ?? false} decks={anki?.decks ?? []} />
            {anki !== null && !anki.connected && <SettingNote>{anki.message}</SettingNote>}
            </Group>
          )}

          {section === "corpus" && (
            <Group title={tr("分词")}>
            <SudachiCard icon={Icons.ruby} onError={onError} onChanged={onChanged} />
            <TokenizeCard icon={Icons.lyrics} onError={onError} onChanged={onChanged} />
            <MigrateCard icon={Icons.db} onError={onError} onChanged={onChanged} />
            </Group>
          )}

          {section === "system" && (
            <Group title={tr("更新")}>
            <UpdateCard
              icons={{ update: Icons.update, clock: Icons.clock, install: Icons.install }}
              onError={onError}
            />
            <DiagnosticsCard icon={Icons.report} onError={onError} />
            <PitchCacheCard icon={Icons.wave} onError={onError} />
            </Group>
          )}

          {section === "about" && (
            <Group title={tr("语料库")}>
            <BuildInfo />
            <SettingCard
              icon={Icons.db}
              title={tr("语料库")}
              description={
                <>
                  {health?.dbPath ?? "—"}
                  {root !== null && <>{tr("（")}{ROOT_SOURCE_LABELS[root.source]}{tr("）")}</>}
                </>
              }
            >
              <span className="muted small">{health === null ? "" : tr("{tracks} 首 · {lyricLines} 行", { tracks: health.tracks, lyricLines: health.lyricLines })}</span>
              <CommandButton icon="folder" label={tr("切换目录…")} onClick={() => void pickLibraryRoot()} />
            </SettingCard>
            {rootNote !== "" && <SettingNote>{tr("{rootNote}，重启后生效。", { rootNote })}</SettingNote>}
            {root !== null && root.remembered !== "" && (
              <SettingNote>{tr("设置里记着的是 {remembered}。", { remembered: root.remembered })}<button
                  className="link-btn"
                  onClick={() => {
                    void api
                      .setLibraryRoot(null)
                      .then(setRootNote)
                      .then(() => api.libraryRoot().then(setRoot))
                      .catch((err: unknown) => onError(String((err as { message?: string })?.message ?? err)));
                  }}
                >{tr("清除")}</button>
              </SettingNote>
            )}
            <SettingCard title={tr("运行状态")} description={tr("分词器、音频设备")}>
              <span className="muted small">
                {health === null
                  ? "—"
                  : [
                      health.tokenizerReady ? tr("分词器就绪") : tr("分词器未就绪"),
                      health.audioReady ? tr("音频可用") : tr("无音频设备"),
                    ].join(" · ")}
              </span>
            </SettingCard>
            </Group>
          )}
        </div>
      </div>
    </div>
  );
}

/** 一页里的一组。和 SettingGroup 一样，只是标题小一号（页名已经在页头了） */
function Group({ title, children }: { title: string; children: ReactNode }) {
  return <SettingGroup title={title}>{children}</SettingGroup>;
}
