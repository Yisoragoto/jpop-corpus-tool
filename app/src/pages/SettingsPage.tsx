/**
 * 设置页。照 Windows 11 设置 / WinUI 的 SettingsCard：一组一组的行，
 * 左边图标、中间标题和说明、右边控件。**改了立刻生效、自动记住**，没有「确定」。
 *
 * 这里不新造一份状态：每一行读写的都是原来那处的存储——
 * 外观和曲库视图在 `settings.ts`，歌词字体字号在 `lyricsDisplay.ts`，
 * 制卡在 `dict/mine.ts`。所以在歌词右上角的「显示」面板里改，这一页跟着变，反过来也一样。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";

import {
  api,
  type AnkiStatus,
  type HealthReport,
  type LibraryRootInfo,
  type LyricsProgress,
} from "../api";
import { CommandButton } from "../components/CommandButton";
import { SettingCard, SettingGroup, SettingNote, SettingSelect, SettingSlider, Switch } from "../components/SettingCard";
import { setDictionariesCollapsed, useCollapsedDictionaries } from "../dict/collapse";
import { DictionaryManager } from "../dict/DictionaryManager";
import { UpdateCard } from "../components/UpdateCard";
import { MigrateCard } from "../components/MigrateCard";
import { MineSettingsCard } from "../dict/MineSettingsCard";
import { lyricFontStack, useFontOptions } from "../fonts";
import {
  DISPLAY_LIMITS,
  FURIGANA_MODE_LABELS,
  resetLyricsDisplay,
  updateLyricsDisplay,
  useLyricsDisplay,
  type FuriganaMode,
} from "../lyricsDisplay";
import { STAGE_BLUR_LIMITS, updateSettings, useAppSettings } from "../settings";

interface Props {
  health: HealthReport | null;
  onError: (message: string) => void;
  /** 跳到别的页（词典管理、刮削这些有自己的整页界面，设置里只放入口） */
  onNavigate: (route: "dict" | "scrape" | "library" | "anki") => void;
  /** 库被改动了（补封面、回填时长），让外壳重新拉总览 */
  onChanged: () => void;
}

const ICON = {
  viewBox: "0 0 24 24",
  width: 16,
  height: 16,
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

const Icons = {
  stage: (
    <svg {...ICON}>
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <path d="M8 19v2h8v-2" />
    </svg>
  ),
  blur: (
    <svg {...ICON}>
      <circle cx="12" cy="12" r="7.5" />
      <path d="M12 4.5v15" />
    </svg>
  ),
  grid: (
    <svg {...ICON}>
      <rect x="4" y="4" width="7" height="7" rx="1.5" />
      <rect x="13" y="4" width="7" height="7" rx="1.5" />
      <rect x="4" y="13" width="7" height="7" rx="1.5" />
      <rect x="13" y="13" width="7" height="7" rx="1.5" />
    </svg>
  ),
  font: (
    <svg {...ICON}>
      <path d="M5 19 10.5 5h3L19 19" />
      <path d="M7.6 14h8.8" />
    </svg>
  ),
  ruby: (
    <svg {...ICON}>
      <path d="M6 18V8h4.5a3 3 0 0 1 0 6H6" />
      <path d="M11 14l4 4" />
      <path d="M15 5h4M17 5v4" />
    </svg>
  ),
  size: (
    <svg {...ICON}>
      <path d="M4 18V7h6M7 7v11" />
      <path d="M14 18v-7h5M16.5 11v7" />
    </svg>
  ),
  dict: (
    <svg {...ICON}>
      <path d="M5 5.5A1.5 1.5 0 0 1 6.5 4H19v16H6.5A1.5 1.5 0 0 1 5 18.5z" />
      <path d="M9 4v16" />
    </svg>
  ),
  card: (
    <svg {...ICON}>
      <rect x="3" y="6" width="18" height="12" rx="2" />
      <path d="M7 10h6M7 14h4" />
    </svg>
  ),
  db: (
    <svg {...ICON}>
      <ellipse cx="12" cy="6" rx="7" ry="3" />
      <path d="M5 6v12c0 1.7 3.1 3 7 3s7-1.3 7-3V6" />
      <path d="M5 12c0 1.7 3.1 3 7 3s7-1.3 7-3" />
    </svg>
  ),
  cover: (
    <svg {...ICON}>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  ),
  clock: (
    <svg {...ICON}>
      <circle cx="12" cy="12" r="8" />
      <path d="M12 7.5V12l3 2" />
    </svg>
  ),
  update: (
    <svg {...ICON}>
      <path d="M20 12a8 8 0 1 1-2.3-5.6" />
      <path d="M20 4v4h-4" />
    </svg>
  ),
  install: (
    <svg {...ICON}>
      <path d="M12 3v11" />
      <path d="M8 10.5l4 4 4-4" />
      <path d="M4 18.5h16" />
    </svg>
  ),
  wave: (
    <svg {...ICON}>
      <path d="M4 12h2l2-5 3 10 3-13 3 8h3" />
    </svg>
  ),
  lyrics: (
    <svg {...ICON}>
      <path d="M4 6h10M4 11h16M4 16h12" />
      <circle cx="18" cy="6" r="2" />
    </svg>
  ),
};

/** 作业刚开始、第一条进度还没到时显示的占位。总数未知，所以是 0/0。 */
const EMPTY_PROGRESS: LyricsProgress = {
  done: 0,
  total: 0,
  songId: "",
  title: "正在准备…",
  status: "",
  source: "",
  matched: "",
  lyricLines: 0,
  message: "",
  filled: 0,
  notFound: 0,
  failed: 0,
  finished: false,
  cancelled: false,
};

/** 语料库是怎么找到的，如实说 */
const ROOT_SOURCE_LABELS: Record<LibraryRootInfo["source"], string> = {
  env: "来自 JPOP_CORPUS_HOME",
  settings: "在设置里选的",
  nextToExe: "在程序旁边找到的",
  workingDir: "当前工作目录",
  default: "默认数据目录",
};

export function SettingsPage({ health, onError, onNavigate, onChanged }: Props) {
  const settings = useAppSettings();
  const display = useLyricsDisplay();
  const fonts = useFontOptions(true, onError);
  const [anki, setAnki] = useState<AnkiStatus | null>(null);
  const [root, setRoot] = useState<LibraryRootInfo | null>(null);
  const [rootNote, setRootNote] = useState("");
  const collapsedDicts = useCollapsedDictionaries();
  const [busy, setBusy] = useState<null | "covers" | "durations">(null);
  const [note, setNote] = useState("");
  /** 缺歌词的歌数。null = 还没数过 */
  const [missingLyrics, setMissingLyrics] = useState<number | null>(null);
  const [lyricsJob, setLyricsJob] = useState<LyricsProgress | null>(null);

  useEffect(() => {
    void api.ankiStatus().then(setAnki).catch(() => undefined);
    void api.libraryRoot().then(setRoot).catch(() => undefined);
  }, []);

  const countMissingLyrics = useCallback(() => {
    void api
      .lyricsMissing()
      .then((rows) => setMissingLyrics(rows.length))
      .catch(() => setMissingLyrics(null));
  }, []);

  // 进度事件在**后台线程**里发，设置页可能这会儿没挂着（用户切走了）。
  // 所以收到 finished 时重新数一遍，而不是依赖界面一直在。
  const onChangedRef = useRef(onChanged);
  onChangedRef.current = onChanged;
  useEffect(() => {
    countMissingLyrics();
    void api.lyricsFillRunning().then((running) => {
      if (running) setLyricsJob({ ...EMPTY_PROGRESS });
    });
    const off = listen<LyricsProgress>("lyrics://progress", (event) => {
      setLyricsJob(event.payload);
      if (event.payload.finished) {
        countMissingLyrics();
        onChangedRef.current();
      }
    });
    return () => {
      void off.then((f) => f());
    };
  }, [countMissingLyrics]);

  const fillLyrics = useCallback(async () => {
    setNote("");
    try {
      setLyricsJob({ ...EMPTY_PROGRESS });
      await api.lyricsFillStart();
    } catch (err) {
      setLyricsJob(null);
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [onError]);

  /** 换语料库目录。**只写设置**，要重启才生效——连接和放行范围都是启动时定下的 */
  const pickLibraryRoot = useCallback(async () => {
    try {
      const picked = await openDialog({ directory: true, title: "选择语料库目录（含 corpus.db）" });
      if (typeof picked !== "string") return;
      setRootNote(await api.setLibraryRoot(picked));
      setRoot(await api.libraryRoot());
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [onError]);

  const importFonts = useCallback(async () => {
    try {
      const picked = await openDialog({
        multiple: true,
        title: "导入字体",
        filters: [{ name: "字体文件", extensions: ["ttf", "otf", "ttc"] }],
      });
      const paths = picked === null ? [] : Array.isArray(picked) ? picked : [picked];
      if (paths.length > 0) fonts.addImported(paths);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [fonts, onError]);

  const backfillCovers = useCallback(async () => {
    setBusy("covers");
    setNote("");
    try {
      const report = await api.scrapeBackfillCovers();
      setNote(
        report.checked === 0
          ? "每一首都有封面了"
          : `检查 ${report.checked} 首：补上 ${report.filled}` +
              (report.noCandidate > 0 ? `，${report.noCandidate} 首没有候选（要重刮）` : "") +
              (report.failed > 0 ? `，${report.failed} 首还是下不到` : ""),
      );
      onChanged();
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(null);
    }
  }, [onChanged, onError]);

  const backfillDurations = useCallback(async () => {
    setBusy("durations");
    setNote("");
    try {
      const report = await api.backfillDurations();
      setNote(`扫描 ${report.scanned} 首：写入 ${report.written}` + (report.failed ? `，失败 ${report.failed}` : ""));
      onChanged();
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(null);
    }
  }, [onChanged, onError]);

  /** 作业在跑：事件还没报 finished */
  const lyricsRunning = lyricsJob !== null && !lyricsJob.finished;

  const fontOptions = [
    { value: "", label: "默认（界面字体）" },
    ...(fonts.options ?? []).map((o) => ({ value: o.value, label: o.label })),
  ];
  // 选中的字体本机没有时也要列出来，否则下拉显示成空白
  if (display.fontFamily !== "" && !fontOptions.some((o) => o.value === display.fontFamily)) {
    fontOptions.push({ value: display.fontFamily, label: `${display.fontFamily}（本机没有）` });
  }
  const fallbackOptions = [{ value: "", label: "不设" }, ...fontOptions.slice(1)];
  if (display.fallbackFamily !== "" && !fallbackOptions.some((o) => o.value === display.fallbackFamily)) {
    fallbackOptions.push({ value: display.fallbackFamily, label: `${display.fallbackFamily}（本机没有）` });
  }

  return (
    <div className="page settings-page">
      <SettingGroup title="外观">
        <SettingCard
          icon={Icons.stage}
          title="全屏歌词的背景用封面虚化"
          description="关掉就用从封面取的一个纯色（原来的样子）。这首歌没有封面时自动回到纯色。"
        >
          <Switch
            checked={settings.stageCoverBlur}
            onChange={(v) => updateSettings({ stageCoverBlur: v })}
            label="全屏背景用封面虚化"
          />
        </SettingCard>
        {settings.stageCoverBlur && (
          <SettingCard icon={Icons.blur} title="虚化强度" description="越大越糊、歌词越清楚；越小越看得出是哪张封面">
            <SettingSlider
              value={settings.stageBlurRadius}
              limits={STAGE_BLUR_LIMITS}
              onChange={(v) => updateSettings({ stageBlurRadius: v })}
            />
          </SettingCard>
        )}
        <SettingCard
          icon={Icons.wave}
          title="播放条上的音频可视化"
          description="播放条顶上那排随音乐跳动的频谱条。关掉之后连采样一起停，不再每 50ms 问一次引擎。"
        >
          <Switch
            checked={settings.spectrum}
            onChange={(v) => updateSettings({ spectrum: v })}
            label="显示音频可视化"
          />
        </SettingCard>
        <SettingCard
          icon={Icons.grid}
          title="曲库的封面网格先显示歌手"
          description="点歌手进去才是他的歌，左上角返回。关掉就是按歌手分组平铺所有歌。"
        >
          <Switch checked={settings.gridByArtist} onChange={(v) => updateSettings({ gridByArtist: v })} label="网格先显示歌手" />
        </SettingCard>
      </SettingGroup>

      <SettingGroup title="歌词">
        <SettingCard icon={Icons.ruby} title="默认显示振假名" description="也可以在歌词右上角随时开关">
          <Switch checked={display.furigana} onChange={(v) => updateLyricsDisplay({ furigana: v })} label="显示振假名" />
        </SettingCard>
        <SettingCard icon={Icons.ruby} title="注音方式" description="只注汉字：思(おも)い出(だ)す；整词注音：思い出す(おもいだす)">
          <SettingSelect<FuriganaMode>
            value={display.furiganaMode}
            options={(Object.keys(FURIGANA_MODE_LABELS) as FuriganaMode[]).map((mode) => ({
              value: mode,
              label: FURIGANA_MODE_LABELS[mode],
            }))}
            onChange={(mode) => updateLyricsDisplay({ furiganaMode: mode, furigana: true })}
          />
        </SettingCard>
        <SettingCard
          icon={Icons.font}
          title="歌词字体"
          description={fonts.options === null ? "正在读取本机字体…" : `本机 ${fonts.options.length} 种`}
        >
          <SettingSelect value={display.fontFamily} options={fontOptions} onChange={(v) => updateLyricsDisplay({ fontFamily: v })} />
        </SettingCard>
        <SettingCard icon={Icons.font} title="备用字体" description="主字体里没有的字用这个显示">
          <SettingSelect
            value={display.fallbackFamily}
            options={fallbackOptions}
            onChange={(v) => updateLyricsDisplay({ fallbackFamily: v })}
          />
        </SettingCard>
        <SettingCard icon={Icons.font} title="导入字体" description="ttf / otf / ttc。只记住文件位置，不复制文件">
          <CommandButton icon="import" label="导入字体…" onClick={() => void importFonts()} />
        </SettingCard>
        <SettingCard icon={Icons.size} title="字号">
          <SettingSlider value={display.fontSize} limits={DISPLAY_LIMITS.fontSize} onChange={(v) => updateLyricsDisplay({ fontSize: v })} />
        </SettingCard>
        <SettingCard icon={Icons.size} title="振假名字号">
          <SettingSlider value={display.rubySize} limits={DISPLAY_LIMITS.rubySize} onChange={(v) => updateLyricsDisplay({ rubySize: v })} />
        </SettingCard>
        <SettingCard icon={Icons.size} title="行距">
          <SettingSlider value={display.lineGap} limits={DISPLAY_LIMITS.lineGap} onChange={(v) => updateLyricsDisplay({ lineGap: v })} />
        </SettingCard>
        <SettingCard icon={Icons.size} title="字距">
          <SettingSlider
            value={display.letterSpacing}
            limits={DISPLAY_LIMITS.letterSpacing}
            onChange={(v) => updateLyricsDisplay({ letterSpacing: v })}
          />
        </SettingCard>
        <SettingCard
          title="预览"
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
          <button className="link-btn" onClick={resetLyricsDisplay}>
            恢复默认
          </button>
        </SettingCard>
      </SettingGroup>

      <DictionaryManager onError={onError} />

      <SettingGroup title="查词">
        <SettingCard icon={Icons.dict} title="折叠记忆" description="查词结果里点词典名可以折叠那一本，之后每次查词都保持折叠">
          <button
            className="tc-btn"
            disabled={collapsedDicts.size === 0}
            onClick={() => setDictionariesCollapsed([...collapsedDicts], false)}
          >
            {collapsedDicts.size === 0 ? "没有折叠的" : `展开这 ${collapsedDicts.size} 本`}
          </button>
        </SettingCard>
      </SettingGroup>

      <SettingGroup title="制卡">
        <MineSettingsCard connected={anki?.connected ?? false} decks={anki?.decks ?? []} />
        {anki !== null && !anki.connected && <SettingNote>{anki.message}</SettingNote>}
      </SettingGroup>

      <SettingGroup title="曲库维护">
        <SettingCard icon={Icons.cover} title="补齐缺失封面" description="只补还没有封面的那些，用刮削时存下的候选，不重新搜索">
          <CommandButton
            icon="photo"
            label={busy === "covers" ? "正在补…" : "补齐封面"}
            onClick={() => void backfillCovers()}
            disabled={busy !== null}
          />
        </SettingCard>
        <SettingCard
          icon={Icons.lyrics}
          title="补齐缺失歌词"
          description={
            missingLyrics === null
              ? "先看音频旁边有没有同名 .lrc，没有再上网搜（网易云）。挑不准的宁可留空"
              : missingLyrics === 0
                ? "每一首都有歌词了"
                : `${missingLyrics} 首还没有歌词。先看音频旁边有没有同名 .lrc，没有再上网搜（网易云）`
          }
        >
          {lyricsRunning ? (
            <CommandButton icon="discard" label="停下" onClick={() => void api.lyricsFillCancel()} />
          ) : (
            <CommandButton
              icon="refresh"
              label="补齐歌词"
              onClick={() => void fillLyrics()}
              disabled={busy !== null || missingLyrics === 0}
            />
          )}
        </SettingCard>
        {lyricsJob !== null && (
          <SettingNote>
            {lyricsJob.finished
              ? `${lyricsJob.cancelled ? "已停下" : "补齐完成"}：补上 ${lyricsJob.filled}，挑不出来 ${lyricsJob.notFound}` +
                (lyricsJob.failed > 0 ? `，失败 ${lyricsJob.failed}` : "") +
                // 整批断了（库打不开、线程起不来）要说出来，不能只报一句「完成」
                (lyricsJob.message !== "" ? `。中断：${lyricsJob.message}` : "")
              : `${lyricsJob.done}/${lyricsJob.total}　${lyricsJob.title}${
                  lyricsJob.status === "filled"
                    ? ` ← ${lyricsJob.matched}（${lyricsJob.lyricLines} 行）`
                    : lyricsJob.status === "notFound"
                      ? " 挑不出来"
                      : lyricsJob.status === "failed"
                        ? ` 失败：${lyricsJob.message}`
                        : ""
                }`}
            {lyricsJob.finished && lyricsJob.notFound > 0 && (
              <>
                {" "}
                挑不出来的那些，可以在曲库里选中那首歌，自己导入一份歌词文件。
              </>
            )}
          </SettingNote>
        )}
        <SettingCard icon={Icons.clock} title="回填时长" description="扫一遍音频文件，把缺的 duration_sec 补上">
          <CommandButton
            icon="refresh"
            label={busy === "durations" ? "正在扫…" : "回填时长"}
            onClick={() => void backfillDurations()}
            disabled={busy !== null}
          />
        </SettingCard>
        <MigrateCard icon={Icons.db} onError={onError} onChanged={onChanged} />
        <SettingCard
          icon={Icons.card}
          title="刮削与人工复核"
          description="识别、补 metadata、歌手照片、专辑封面"
          onClick={() => onNavigate("scrape")}
        >
          <span className="setting-chevron" aria-hidden="true">
            ›
          </span>
        </SettingCard>
        {note !== "" && <SettingNote>{note}</SettingNote>}
      </SettingGroup>

      <SettingGroup title="系统">
        <UpdateCard
          icons={{ update: Icons.update, clock: Icons.clock, install: Icons.install }}
          onError={onError}
        />
      </SettingGroup>

      <SettingGroup title="关于">
        <SettingCard
          icon={Icons.db}
          title="语料库"
          description={
            <>
              {health?.dbPath ?? "—"}
              {root !== null && <>（{ROOT_SOURCE_LABELS[root.source]}）</>}
            </>
          }
        >
          <span className="muted small">{health === null ? "" : `${health.tracks} 首 · ${health.lyricLines} 行`}</span>
          <CommandButton icon="folder" label="切换目录…" onClick={() => void pickLibraryRoot()} />
        </SettingCard>
        {rootNote !== "" && <SettingNote>{rootNote}，重启后生效。</SettingNote>}
        {root !== null && root.remembered !== "" && (
          <SettingNote>
            设置里记着的是 {root.remembered}。
            <button
              className="link-btn"
              onClick={() => {
                void api
                  .setLibraryRoot(null)
                  .then(setRootNote)
                  .then(() => api.libraryRoot().then(setRoot))
                  .catch((err: unknown) => onError(String((err as { message?: string })?.message ?? err)));
              }}
            >
              清除
            </button>
          </SettingNote>
        )}
        <SettingCard title="运行状态" description="分词器、音频设备、变调支持">
          <span className="muted small">
            {health === null
              ? "—"
              : [
                  health.tokenizerReady ? "分词器就绪" : "分词器未就绪",
                  health.audioReady ? "音频可用" : "无音频设备",
                  health.pitchSupported ? "支持变调" : "不支持变调",
                ].join(" · ")}
          </span>
        </SettingCard>
      </SettingGroup>
    </div>
  );
}
