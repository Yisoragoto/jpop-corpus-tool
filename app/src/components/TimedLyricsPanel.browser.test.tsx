/** Browser integration tests: npm run dev -- --port 5186, then /tests/timed-lyrics.html.
 * Uses React's act and the real DOM; no extra packages or backend are required.
 */
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { TimedLyricsPanel } from "./TimedLyricsPanel";
import { LyricsFill } from "./LyricsFill";
import type { AttachedLyrics, TimedSearch, TimedSourceSettings } from "../api";
import "../styles.css";

const rootNode = document.querySelector<HTMLDivElement>("#qa-root")!;
const report = document.querySelector<HTMLPreElement>("#qa-report")!;
const root = createRoot(rootNode);
const browser = window as unknown as {
  __TAURI_INTERNALS__: { invoke: typeof invoke };
  IS_REACT_ACT_ENVIRONMENT: boolean;
};
browser.IS_REACT_ACT_ENVIRONMENT = true;

type Args = Record<string, unknown>;
type Handler = (args: Args) => unknown;
let calls: { command: string; args: Args }[] = [];
let handlers: Record<string, Handler> = {};
let errors: string[] = [];
let changed = 0;
let applied = 0;
let attached = 0;
const settings: TimedSourceSettings = {
  netease: true, qq: true, kugou: true, amll: true, lrclib: true, apple: false,
  appleStorefront: "jp", appleDeveloperTokenSet: true,
};
const targets = [
  { songId: "a", title: "Song A", artist: "Artist A", lyricLines: 20, timedLines: 0 },
  { songId: "b", title: "Song B", artist: "Artist B", lyricLines: 24, timedLines: 8 },
];
const attachment = (songId: string): AttachedLyrics => ({
  songId, source: "qq", matched: "Song", lyricLines: 20, tokens: 10, credits: 0, correctionsRestored: 0, lrcPath: "test.qrc",
});
const searchResult = (songId: string): TimedSearch => ({
  songId, title: `Song ${songId.toUpperCase()}`, currentLines: 20, currentTimedLines: 0,
  candidates: [
    { id: `${songId}-qq`, source: "qq", format: "QRC", title: `Candidate ${songId}`, artist: "Artist", lineCount: 20, timedLineCount: 18, preview: ["one", "two", "three", "four", "five", "six"], matchNote: "check the recording" },
    { id: `${songId}-lrclib`, source: "lrclib", format: "plain", title: "Alternate", artist: "Artist", lineCount: 22, timedLineCount: 0, preview: [], matchNote: "" },
  ],
  warnings: [{ source: "kugou", message: "HTTP 503: retry later" }],
});

function invoke(command: string, args: Args = {}): Promise<unknown> {
  // Clone at the IPC boundary: the UI deliberately removes transient secret fields after saving.
  calls.push({ command, args: structuredClone(args) });
  try { return Promise.resolve(handlers[command]?.(args)); }
  catch (error) { return Promise.reject(error); }
}
browser.__TAURI_INTERNALS__ = { invoke };

function reset() {
  calls = []; errors = []; changed = 0; applied = 0; attached = 0;
  handlers = {
    lyrics_timed_targets: () => targets,
    lyrics_sources_get: () => ({ ...settings }),
    lyrics_sources_set: () => ({ ...settings }),
    lyrics_timed_search: (args) => searchResult(String(args.songId)),
    lyrics_timed_apply: (args) => attachment(String(args.songId)),
    "plugin:dialog|open": () => "test.ttml",
    lyrics_import_file: (args) => attachment(String(args.songId)),
  };
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
function count(command: string) { return calls.filter((call) => call.command === command).length; }
function button(text: string): HTMLButtonElement {
  const found = [...rootNode.querySelectorAll("button")].find((element) => element.textContent === text);
  assert(found, `Button not found: ${text}`);
  return found;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const onError = (message: string) => { errors.push(message); };
const onChanged = () => { ++changed; };
const onApplied = () => { ++applied; };
const panel = (songId?: string) => <StrictMode><TimedLyricsPanel {...(songId === undefined ? {} : { songId })} onError={onError} onChanged={onChanged} onApplied={onApplied} /></StrictMode>;
async function render(node: ReactNode) { await act(async () => { root.render(node); }); }
async function click(element: HTMLElement, twice = false) {
  await act(async () => { element.click(); if (twice) element.click(); });
}
async function selectSong(value: string) {
  await act(async () => {
    const select = rootNode.querySelector("select")!;
    select.value = value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
async function submit() {
  await act(async () => { rootNode.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })); });
}
async function test(name: string, work: () => Promise<void>) {
  await render(null);
  reset();
  try { await work(); report.textContent += `\nPASS ${name}`; }
  catch (error) { report.textContent += `\nFAIL ${name}: ${error instanceof Error ? error.message : "unknown error"}`; }
}

async function run() {
  report.textContent = "Timed lyrics browser integration tests";
  await test("roster is read-only; settings default Apple off and never echo saved tokens", async () => {
    await render(panel());
    assert(rootNode.querySelectorAll("select option").length === 2, "missing roster options");
    assert(count("lyrics_timed_search") === 0 && count("lyrics_timed_apply") === 0, "automatic network/write request");
    assert(!rootNode.querySelector<HTMLDetailsElement>(".timed-sources")!.open, "settings should be collapsed");
    const checks = rootNode.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    assert(checks.length === 6 && !checks[5]!.checked && checks[4]!.checked && checks[0]!.checked, "wrong source defaults");
    assert([...rootNode.querySelectorAll<HTMLInputElement>('input[type="password"]')].every((input) => input.value === ""), "saved token echoed");
    assert(rootNode.querySelectorAll('input[type="password"]').length === 1, "unexpected user-token input");
  });
  await test("search shows multiple candidates, source failures, exact counts and five preview lines without applying", async () => {
    await render(panel("a"));
    await click(button("搜索逐字歌词"));
    assert(rootNode.querySelectorAll(".timed-candidate").length === 2, "missing candidates");
    assert(rootNode.textContent!.includes("HTTP 503") && rootNode.textContent!.includes("QRC"), "missing source diagnostics");
    assert(rootNode.textContent!.includes("真实逐字 0 行"), "zero word timings misreported");
    assert(rootNode.querySelector("pre")!.textContent === "one\ntwo\nthree\nfour\nfive", "wrong inline preview");
    assert(count("lyrics_timed_apply") === 0 && count("lyrics_timed_targets") === 0, "implicit apply or roster scan");
  });
  await test("duplicate search is blocked; changing song discards the old response", async () => {
    const slow = deferred<TimedSearch>();
    handlers.lyrics_timed_search = (args) => args.songId === "a" ? slow.promise : searchResult("b");
    await render(panel());
    await click(button("搜索逐字歌词"), true);
    assert(count("lyrics_timed_search") === 1, "duplicate search request");
    await selectSong("b");
    await click(button("搜索逐字歌词"));
    await act(async () => { slow.resolve(searchResult("a")); });
    assert(rootNode.textContent!.includes("Candidate b") && !rootNode.textContent!.includes("Candidate a"), "stale song response displayed");
    const before = calls.length;
    await render(<StrictMode><TimedLyricsPanel onError={(message) => onError(message)} onChanged={() => onChanged()} /></StrictMode>);
    assert(calls.length === before, "new callback identities retriggered requests");
  });
  await test("adoption is explicit, deduplicated, disables competing actions and notifies once", async () => {
    const slow = deferred<AttachedLyrics>();
    handlers.lyrics_timed_apply = () => slow.promise;
    await render(panel("a"));
    await click(button("搜索逐字歌词"));
    await click(button("采用"), true);
    assert(count("lyrics_timed_apply") === 1 && changed === 0 && applied === 0, "duplicate/premature adoption");
    assert([...rootNode.querySelectorAll<HTMLButtonElement>(".timed-search button")].every((element) => element.disabled), "competing action enabled");
    await act(async () => { slow.resolve(attachment("a")); });
    assert(Number(changed) === 1 && Number(applied) === 1, "wrong callback count");
    assert(count("lyrics_timed_search") === 1 && rootNode.querySelectorAll(".timed-candidate").length === 0, "apply triggered another search");
  });
  await test("external song change discards an old apply completion", async () => {
    const slow = deferred<AttachedLyrics>();
    handlers.lyrics_timed_apply = () => slow.promise;
    await render(panel("a"));
    await click(button("搜索逐字歌词"));
    await click(button("采用"));
    await render(panel("b"));
    await act(async () => { slow.resolve(attachment("a")); });
    assert(changed === 0 && applied === 0 && !rootNode.textContent!.includes("已采用"), "old apply affected new song");
  });
  await test("source save omits empty secrets; explicit clear sends only that empty secret", async () => {
    await render(panel("a"));
    await submit();
    let config = calls.find((call) => call.command === "lyrics_sources_set")!.args.config as Args;
    assert(!("appleDeveloperToken" in config) && !("appleMusicUserToken" in config), "blank fields cleared saved credentials");
    await click(button("清除 Developer Token"));
    await submit();
    config = calls.filter((call) => call.command === "lyrics_sources_set").at(-1)!.args.config as Args;
    assert(config.appleDeveloperToken === "" && !("appleMusicUserToken" in config), "incorrect explicit clear payload");
    assert(count("lyrics_timed_search") === 0, "saving sources automatically searched");
  });
  await test("typed credentials disappear before IPC finishes; failed saves never expose the raw error", async () => {
    const slow = deferred<TimedSourceSettings>();
    handlers.lyrics_sources_set = () => slow.promise;
    await render(panel("a"));
    const passwords = rootNode.querySelectorAll<HTMLInputElement>('input[type="password"]');
    passwords[0]!.value = "fake-developer-secret";

    await submit();
    assert([...passwords].every((input) => input.value === ""), "credentials remain in the inputs");
    const config = calls.find((call) => call.command === "lyrics_sources_set")!.args.config as Args;
    assert(config.appleDeveloperToken === "fake-developer-secret" && !("appleMusicUserToken" in config), "credentials were not sent");
    await submit();
    assert(count("lyrics_sources_set") === 1, "duplicate save");
    await act(async () => { slow.reject(new Error("bad fake-developer-secret fake-user-secret")); });
    assert(errors.length === 1 && !errors[0]!.includes("fake-"), "raw secret-bearing error exposed");
    assert(!rootNode.textContent!.includes("fake-"), "secret echoed as text");
  });
  await test("source changes invalidate old search results and late responses", async () => {
    const slow = deferred<TimedSearch>();
    handlers.lyrics_timed_search = () => slow.promise;
    await render(panel("a"));
    await click(button("搜索逐字歌词"));
    await submit();
    await act(async () => { slow.resolve(searchResult("a")); });
    assert(rootNode.querySelectorAll(".timed-candidate").length === 0, "old-source response survived save");
  });
  await test("expired candidates are cleared and failures are not disguised as no results", async () => {
    await render(panel("a"));
    await click(button("搜索逐字歌词"));
    handlers.lyrics_timed_apply = () => { throw new Error("stale candidate"); };
    await click(button("采用"));
    assert(errors[0] === "stale candidate" && rootNode.querySelectorAll(".timed-candidate").length === 0, "expired candidate still usable");
    handlers.lyrics_timed_search = () => ({ ...searchResult("a"), candidates: [] });
    await click(button("搜索逐字歌词"));
    assert(rootNode.textContent!.includes("HTTP 503") && rootNode.textContent!.includes("各来源说明"), "source failure hidden");
  });
  await test("LyricsFill attaches once for existing lyrics and keeps all supported import extensions", async () => {
    await render(<LyricsFill songId="a" hasLyrics onAttached={() => { ++attached; }} onError={onError} />);
    await act(async () => {
      const details = rootNode.querySelector<HTMLDetailsElement>(".lyrics-timed-entry")!;
      details.open = true;
      details.dispatchEvent(new Event("toggle"));
    });
    await click(button("搜索逐字歌词"));
    await click(button("采用"));
    assert(attached === 1 && count("lyrics_timed_search") === 1, "onAttached duplicated or search looped");
    await act(async () => {
      const details = rootNode.querySelector<HTMLDetailsElement>(".lyrics-timed-entry")!;
      details.open = false;
      details.dispatchEvent(new Event("toggle"));
    });
    await click(button("换一份歌词…"));
    const dialog = calls.find((call) => call.command === "plugin:dialog|open");
    const extensions = (dialog?.args.options as { filters: { extensions: string[] }[] }).filters[0]!.extensions;
    assert(["lrc", "txt", "yrc", "qrc", "krc", "ttml"].every((extension) => extensions.includes(extension)), "missing import extension");
    assert(Number(attached) === 2, "file import did not attach");
  });
  await render(panel());
  const failed = report.textContent!.includes("FAIL");
  report.textContent += failed ? "\nFAILED" : "\nALL 10 BROWSER TESTS PASSED";
  document.title = failed ? "FAIL — timed lyrics" : "PASS — timed lyrics";
}
/** Dedicated, clearly labelled fixture for layout screenshots; never contacts a real source. */
async function renderLayout() {
  reset();
  handlers.lyrics_timed_targets = () => [
    { songId: "test-layout", title: "【测试曲目】逐字排版样本", artist: "【测试歌手】虚构数据", lyricLines: 20, timedLines: 0 },
  ];
  handlers.lyrics_timed_search = () => ({
    songId: "test-layout", title: "【测试曲目】逐字排版样本", currentLines: 20, currentTimedLines: 0,
    candidates: [
      { id: "test-qrc", source: "qq", format: "QRC", title: "【测试候选 1】逐字排版样本", artist: "【测试歌手】虚构数据", lineCount: 20, timedLineCount: 18, preview: ["【测试占位 01】此处仅检查预览排版。", "【测试占位 02】这不是歌曲歌词。", "【测试占位 03】时间与行数均为模拟。"], matchNote: "【测试匹配说明】请核对测试录音版本；不对应真实曲目。" },
      { id: "test-ttml", source: "amll", format: "TTML", title: "【测试候选 2】另一种格式的排版样本", artist: "【测试歌手】虚构数据", lineCount: 22, timedLineCount: 22, preview: ["【测试占位 A】用于第二个候选的布局检查。", "【测试占位 B】不含任何真实歌词文本。", "【测试占位 C】仅展示模拟返回的数据。"], matchNote: "【测试匹配说明】第二个虚构候选，用于比较格式与逐字行数。" },
    ],
    warnings: [{ source: "kugou", message: "【模拟来源说明】测试网络失败，不代表真实来源状态。" }],
  });
  report.hidden = true;
  document.title = "逐字歌词布局测试 · Mock IPC · 非真实数据";
  await render(
    <main className="qa-layout">
      <header>
        <h1>逐字歌词 · 布局测试</h1>
        <p className="qa-label">测试组件：TimedLyricsPanel（Mock IPC）</p>
        <p>全部曲目、歌手、候选、预览和凭据配置状态均为模拟数据；不使用真实歌词、真实令牌或外部网络。</p>
      </header>
      {panel()}
    </main>,
  );
}

const layoutMode = new URLSearchParams(location.search).has("layout");
void (layoutMode ? renderLayout() : run()).catch((error: unknown) => { report.hidden = false; report.textContent += `\nFATAL: ${error instanceof Error ? error.message : "unknown error"}`; });
