/**
 * 应用外壳：导航 + 页面切换 + 常驻播放条。
 *
 * **播放条常驻、状态在 Rust 侧**，所以切页不会中断播放——
 * 这是要求书第十八条那句「不应该感觉自己在几个独立的软件之间切换」
 * 在架构上的落地，不是靠 UI 技巧堆出来的。
 *
 * Corpus First 体现在首页的**版面顺序**上：先语料概览、再高频词入口，
 * 收听记录排在后面。打开软件第一眼看到的是「这个库里有什么」，
 * 不是「你最近在听什么」。
 */

import { useCallback, useEffect, useMemo, useState } from "react";

import { api, NO_DICTIONARY, type HealthReport, type Overview, type QuickHit } from "./api";
import { CommandPalette, type PaletteCommand, type PaletteMode } from "./components/CommandPalette";
import { NavIcon } from "./components/NavIcons";
import { invalidatePerformers } from "./components/PerformerPicker";
import { Player } from "./components/Player";
import { SudachiBanner } from "./components/SudachiBanner";
import { warmFontOptions } from "./fonts";
import { useAppSettings } from "./settings";
import { Stat } from "./components/Stat";
import { AnalyticsPage } from "./pages/AnalyticsPage";
import { HomePage } from "./pages/HomePage";
import { ImportPage } from "./pages/ImportPage";
import { AnkiPage } from "./pages/AnkiPage";
import { DictionaryPage } from "./pages/DictionaryPage";
import { ScrapePage } from "./pages/ScrapePage";
import { ExplorerPage } from "./pages/ExplorerPage";
import { KwicPage, type KwicRequest } from "./pages/KwicPage";
import { LibraryPage } from "./pages/LibraryPage";
import { SettingsPage } from "./pages/SettingsPage";
import { useLibrary } from "./useLibrary";
import { useHotkeys } from "./useHotkeys";
import { usePlaybackState, useSpectrum } from "./usePlayback";

type Route = "home" | "kwic" | "library" | "explorer" | "analytics" | "import" | "scrape" | "anki" | "dict" | "settings";

const ROUTES: { key: Route; label: string; hint: string }[] = [
  { key: "home", label: "首页", hint: "语料概览 · 收听记录 · 收藏" },
  { key: "kwic", label: "检索", hint: "KWIC 语境检索与歌词全文" },
  { key: "library", label: "曲库", hint: "曲目 · 歌词 · 语料" },
  { key: "explorer", label: "人物", hint: "演唱 / 作曲 / 作词 / 编曲" },
  { key: "analytics", label: "分析", hint: "总览 · 时间线 · 词频统计 · 语料报告" },
  { key: "import", label: "导入", hint: "扫描目录 · 复核 · 写入语料" },
  { key: "scrape", label: "刮削", hint: "识别 · 补 metadata · 封面" },
  { key: "anki", label: "Anki", hint: "选词 · 用语料例句做卡片" },
  { key: "dict", label: "词典", hint: "查词 · 释义 · 音调 · 词频" },
];

/** 设置不排在导航列表里，钉在左下角——和 Windows 11 设置、Niratan 一样 */
const SETTINGS_ROUTE = { key: "settings" as const, label: "设置", hint: "外观 · 歌词 · 词典 · 制卡 · 曲库维护 · 更新" };

type Status = { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready" };

export default function App() {
  const [status, setStatus] = useState<Status>({ kind: "loading" });
  const [health, setHealth] = useState<HealthReport | null>(null);
  const [overview, setOverview] = useState<Overview | null>(null);
  const [route, setRoute] = useState<Route>("home");
  // 导航栏展开 / 收起（照 WinUI 的 NavigationView）。窄屏和想多留内容宽度时收起
  const [navOpen, setNavOpen] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);

  const settings = useAppSettings();
  const audioReady = health?.audioReady ?? false;
  const playback = usePlaybackState(audioReady);
  // 频谱关掉时连轮询一起停：没人看的东西不值得每 50ms 问一次引擎
  const spectrum = useSpectrum(audioReady && settings.spectrum, playback.playState === "playing");
  /** 「进全屏歌词」的请求。曲库页收到之后自己清掉（和跨页跳转同一个路子） */
  const [stageWanted, setStageWanted] = useState(false);
  const openStage = useCallback(() => {
    setRoute("library");
    setStageWanted(true);
  }, []);
  const consumeStage = useCallback(() => setStageWanted(false), []);

  // 启动时检查更新（设置里可关）。只问版本号；开了「下载后自动安装」才会接着下。
  // 放在这里而不是设置页：用户不进设置也该知道有新版。
  useEffect(() => {
    if (!settings.autoCheckUpdates) return;
    let alive = true;
    const timer = setTimeout(() => {
      void api
        .updateCheck()
        .then(async (status) => {
          if (!alive || !status.updateAvailable || status.latest === null) return;
          const asset = status.latest.installer;
          if (!settings.autoInstallUpdates || asset === null) {
            setNotice(`有新版本 ${status.latest.version}，到「设置 → 系统」里更新`);
            return;
          }
          // 用户明确开过「下载后自动安装」才走到这里；装之前先说一声要关应用
          setNotice(`正在下载新版本 ${status.latest.version}，装好会自动重启应用`);
          const path = await api.updateDownload(asset);
          if (!alive) return;
          await api.updateInstall(path);
        })
        .catch(() => undefined); // 没网、被限流都不该在启动时打扰用户
    }, 4000);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
    // 开关改了之后不重新跑：这一轮已经查过了
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 字体探测很贵（本机 401 个名字冷着跑 1.8 秒），空闲时先备好，
  // 免得用户点开「显示」面板或设置页时卡在那里。结果会存进 localStorage。
  useEffect(warmFontOptions, []);

  const onError = useCallback((message: string) => setNotice(message), []);
  const [library, actions] = useLibrary(audioReady, onError);

  useEffect(() => {
    (async () => {
      try {
        const [h, o, t] = await Promise.all([
          api.health(),
          api.overview(),
          api.listTracks(1000),
        ]);
        setHealth(h);
        setOverview(o);
        actions.setTracks(t);
        setStatus({ kind: "ready" });
      } catch (err) {
        setStatus({
          kind: "error",
          message: err instanceof Error ? err.message : String(err),
        });
      }
    })();
    // actions.setTracks 是 useState 的 setter，引用稳定；只需要跑一次
  }, [actions.setTracks]);

  const [palette, setPalette] = useState<PaletteMode | null>(null);
  // 命令面板跳到人物页时要聚焦的人。页面消费掉后清空。
  const [focusPersonId, setFocusPersonId] = useState<number | null>(null);

  useHotkeys({
    onQuickSearch: () => setPalette("search"),
    onCommandPalette: () => setPalette("command"),
    audioReady,
    positionSec: playback.positionSec,
    volume: playback.volume,
  });

  const goLibrary = useCallback(() => setRoute("library"), []);
  // 分析页点「检索」：带着词跳到检索页，检索页用掉后清空
  const [kwicRequest, setKwicRequest] = useState<KwicRequest | null>(null);
  const searchWord = useCallback((lemma: string) => {
    setKwicRequest({ keyword: lemma, field: "lemma", nonce: Date.now() });
    setRoute("kwic");
  }, []);
  const consumeKwicRequest = useCallback(() => setKwicRequest(null), []);
  // 维护操作改了库之后要刷新总览，否则数字还是旧的；歌手列表也可能变了
  const refreshOverview = useCallback(() => {
    invalidatePerformers();
    api.overview().then(setOverview).catch(() => undefined);
  }, []);

  // 导入之后曲库和语料统计都变了，两个都得重拉
  const onImported = useCallback(() => {
    api.listTracks().then(actions.setTracks).catch(() => undefined);
    refreshOverview();
  }, [actions, refreshOverview]);

  // 面板里选中一条结果 → 跳到对应的地方。
  // 每种结果的落点不同，这个映射就是「搜索不是死胡同」的具体实现。
  const onPickHit = useCallback(
    async (hit: QuickHit) => {
      switch (hit.kind) {
        case "track":
          setRoute("library");
          await actions.openTrackById(hit.songId, { autoPlay: false });
          break;
        case "lyric":
          setRoute("library");
          await actions.openTrackById(hit.songId, {
            scrollTo: hit.utteranceId,
            seekTo: hit.timeSec,
            autoPlay: false,
          });
          break;
        case "word":
          await actions.openWord(hit.lemma);
          setRoute("library");
          break;
        case "person":
          setFocusPersonId(hit.personId);
          setRoute("explorer");
          break;
        case "album":
          // 专辑没有独立页面；把曲库列表限定到这张专辑，
          // 这也正好补上了 Artist → Album → Track 的中间一级
          await actions.setAlbumFilter({ albumId: hit.albumId, title: hit.title });
          setRoute("library");
          break;
      }
    },
    [actions],
  );

  const paletteCommands = useMemo<PaletteCommand[]>(
    () => [
      {
        id: "play",
        label: playback.playState === "playing" ? "暂停" : "播放",
        hint: "Space",
        ...(audioReady ? {} : { disabled: "没有音频设备" }),
        run: () => void api.audioToggle(),
      },
      {
        id: "loop-off",
        label: "取消单句循环",
        ...(playback.loopRegion ? {} : { disabled: "当前没有循环" }),
        run: () => void api.audioSetLoop(),
      },
      { id: "go-home", label: "打开 首页", hint: "语料概览", run: () => setRoute("home") },
      { id: "go-kwic", label: "打开 检索", hint: "KWIC", run: () => setRoute("kwic") },
      { id: "go-library", label: "打开 曲库", run: () => setRoute("library") },
      { id: "go-explorer", label: "打开 人物", run: () => setRoute("explorer") },
      { id: "go-analytics", label: "打开 分析", run: () => setRoute("analytics") },
      { id: "go-import", label: "打开 导入", hint: "扫描目录并写入语料", run: () => setRoute("import") },
      { id: "go-scrape", label: "打开 刮削", hint: "识别 · metadata · 封面", run: () => setRoute("scrape") },
      { id: "go-anki", label: "打开 Anki", hint: "用语料例句做卡片", run: () => setRoute("anki") },
      { id: "go-settings", label: "打开 设置", hint: "外观 · 歌词 · 词典 · 制卡", run: () => setRoute("settings") },
      {
        id: "search",
        label: "全局搜索…",
        hint: "Ctrl/Cmd+K",
        run: () => setPalette("search"),
      },
      {
        id: "backfill",
        label: "回填时长",
        hint: "扫描音频文件补 duration_sec",
        run: async () => {
          const report = await api.backfillDurations();
          setNotice(
            `扫描 ${report.scanned} 首：写入 ${report.written}` +
              (report.failed ? `，失败 ${report.failed}` : ""),
          );
          refreshOverview();
        },
      },
    ],
    [playback.playState, playback.loopRegion, audioReady, refreshOverview],
  );

  if (status.kind === "loading") {
    return <div className="center muted">正在打开语料库…</div>;
  }
  if (status.kind === "error") {
    return (
      <div className="center">
        <div className="error-box">
          <h2>启动失败</h2>
          <p>{status.message}</p>
          <p className="muted">
            先跑 <code>python scripts/migrate_db.py</code> 和{" "}
            <code>python scripts/backfill_library.py</code>
          </p>
        </div>
      </div>
    );
  }

  const current = [...ROUTES, SETTINGS_ROUTE].find((r) => r.key === route) ?? { key: route, label: "", hint: "" };

  return (
    <div className="app">
      <aside className={`sidebar ${navOpen ? "" : "collapsed"}`}>
        <div className="sidebar-head">
          <button
            className="icon-btn nav-toggle"
            onClick={() => setNavOpen((v) => !v)}
            title={navOpen ? "收起导航" : "展开导航"}
          >
            <NavIcon name="menu" />
          </button>
          <span className="brand-text">JPOP Corpus</span>
        </div>

        <nav className="nav">
          {ROUTES.map((r) => (
            <button
              key={r.key}
              className={route === r.key ? "on" : ""}
              onClick={() => setRoute(r.key)}
              title={`${r.label} · ${r.hint}`}
            >
              <NavIcon name={r.key} />
              <span className="nav-label">{r.label}</span>
            </button>
          ))}
        </nav>

        <div className="sidebar-foot">
          <button className="nav-search" onClick={() => setPalette("search")} title="搜索全库 (Ctrl K)">
            <NavIcon name="search" />
            <span className="nav-label">搜索全库</span>
            <kbd>Ctrl K</kbd>
          </button>
          <nav className="nav">
            <button
              className={route === "settings" ? "on" : ""}
              onClick={() => setRoute("settings")}
              title={`${SETTINGS_ROUTE.label} · ${SETTINGS_ROUTE.hint}`}
            >
              <NavIcon name="settings" />
              <span className="nav-label">{SETTINGS_ROUTE.label}</span>
            </button>
          </nav>
        </div>
      </aside>

      <main className="main">
        <header className="cmdbar">
          <div className="page-head">
            <h1>{current.label}</h1>
            <span className="muted small">{current.hint}</span>
          </div>
          {overview && (
            <div className="stats">
              <Stat label="曲目" value={overview.tracks} />
              <Stat label="歌词行" value={overview.lyricLines} />
              <Stat label="词汇" value={overview.vocabulary} />
            </div>
          )}
        </header>

        {health && !health.audioReady && (
          <div className="banner">
            没有可用的音频输出设备，播放已禁用；歌词与语料浏览不受影响。
          </div>
        )}
        {/* 没有词典是唯一一条「能当场解决」的报错：横幅上直接给下载按钮，别让用户去设置里找 */}
        {notice && notice.includes(NO_DICTIONARY) && (
          <SudachiBanner
            message={notice}
            onDone={() => {
              setNotice(null);
              void api.health().then(setHealth).catch(() => undefined);
            }}
            onDismiss={() => setNotice(null)}
          />
        )}
        {notice && !notice.includes(NO_DICTIONARY) && (
          <div className="banner error" onClick={() => setNotice(null)}>
            {notice}（点击关闭）
          </div>
        )}

        <div className="body">
        {route === "home" && (
          <HomePage
            actions={actions}
            onNavigate={setRoute}
            onError={onError}
            playbackSongId={playback.songId}
          />
        )}
        {route === "kwic" && (
          <KwicPage
            actions={actions}
            onNavigate={goLibrary}
            onError={onError}
            request={kwicRequest}
            onRequestConsumed={consumeKwicRequest}
          />
        )}
        {route === "library" && (
          <LibraryPage
            library={library}
            actions={actions}
            playback={playback}
            audioReady={audioReady}
            onAudioError={onError}
            onLibraryChanged={refreshOverview}
            stageWanted={stageWanted}
            onStageConsumed={consumeStage}
          />
        )}
        {route === "explorer" && (
          <ExplorerPage
            actions={actions}
            onNavigate={goLibrary}
            onError={onError}
            focusPersonId={focusPersonId}
            onFocusConsumed={() => setFocusPersonId(null)}
          />
        )}
        {route === "import" && (
          <ImportPage onError={onError} onImported={onImported} />
        )}
        {route === "scrape" && (
          <ScrapePage onError={onError} onChanged={onImported} />
        )}
        {route === "anki" && <AnkiPage onError={onError} />}
        {route === "settings" && (
          <SettingsPage health={health} onError={onError} onNavigate={setRoute} onChanged={refreshOverview} />
        )}
        {route === "dict" && <DictionaryPage onError={onError} onOpenSettings={() => setRoute("settings")} />}
        {route === "analytics" && (
          <AnalyticsPage
            overview={overview}
            onOverviewChanged={refreshOverview}
            actions={actions}
            onNavigate={goLibrary}
            onSearchWord={searchWord}
            onError={onError}
          />
        )}
        </div>
      </main>

      {palette && (
        <CommandPalette
          mode={palette}
          commands={paletteCommands}
          onClose={() => setPalette(null)}
          onPickHit={onPickHit}
        />
      )}

      <Player
        state={playback}
        spectrum={spectrum}
        title={library.selected?.title ?? ""}
        artist={library.selected?.artist ?? ""}
        coverPath={library.selected?.coverPath}
        disabled={!audioReady}
        showSpectrum={settings.spectrum}
        // 没选中曲目时没有歌词可全屏，卡片就不可点
        onOpenStage={library.selected ? openStage : null}
        onError={onError}
      />
    </div>
  );
}
