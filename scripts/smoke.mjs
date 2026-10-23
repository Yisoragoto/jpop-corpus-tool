// WebView2 的真实键盘和 DOM 入口；轮询条件，不用固定等待冒充操作完成。
// node scripts/smoke.mjs [--scenario all|mouse|keyboard|normal] [--exe path] [--out dir]
// 兼容 release-check.ps1：node scripts/smoke.mjs <port> [output.json]
import { mkdirSync, mkdtempSync, copyFileSync, existsSync, statSync, writeFileSync, openSync, closeSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:net';
import assert from 'node:assert/strict';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const pause = ms => new Promise(r => setTimeout(r, ms));
async function until(label, check, timeout = 30000) {
  const deadline = Date.now() + timeout;
  let last;
  while (Date.now() < deadline) {
    try { const value = await check(); if (value) return value; last = value; }
    catch (error) { last = error.cause?.message ?? error.message; }
    await pause(100);
  }
  throw new Error(`${label} 超时：${JSON.stringify(last)}`);
}

async function connect(port) {
  const target = await until('WebView2 页面', async () => {
    const list = await (await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(1000) })).json();
    return list.find(t => t.type === 'page' && t.url.startsWith('http'));
  }, 60000);
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  let next = 0;
  const pending = new Map();
  ws.onmessage = ({ data }) => {
    const msg = JSON.parse(data);
    if (pending.has(msg.id)) {
      const { resolve, reject, timer } = pending.get(msg.id);
      clearTimeout(timer); pending.delete(msg.id);
      if (msg.error) reject(new Error(JSON.stringify(msg.error))); else resolve(msg.result);
    }
  };
  await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++next;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP ${method} 超时`)); }, 30000);
    pending.set(id, { resolve, reject, timer });
    ws.send(JSON.stringify({ id, method, params }));
  });
  const run = async expression => {
    const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? JSON.stringify(result.exceptionDetails));
    return result.result?.value;
  };
  const invoke = (cmd, params = {}) => run(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)},${JSON.stringify(params)})`);
  const key = async (key, code, vk, text) => {
    await send('Input.dispatchKeyEvent', { type: 'keyDown', key, code, windowsVirtualKeyCode: vk, ...(text ? { text } : {}) });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: vk });
  };
  const tabTo = async expression => {
    // 必须真的按 Tab，不能 focus() 绕过键盘可达性。
    for (let i = 0; i < 100; i++) {
      await key('Tab', 'Tab', 9);
      if (await run(expression)) return;
    }
    throw new Error(`Tab 无法到达：${await run('document.activeElement?.outerHTML')}`);
  };
  return { run, invoke, send, key, tabTo, close: () => ws.close() };
}

if (/^\d+$/.test(args[0] ?? '')) {
  const c = await connect(args[0]);
  try {
    const health = await c.invoke('health');
    const dict = await c.invoke('tokenizer_status');
    const out = { href: await c.run('location.href'), navButtons: await c.run("[...document.querySelectorAll('.nav button')].map(b=>b.textContent.trim())"), health, dict: { ready: dict.ready, source: dict.source } };
    if (args[1]) writeFileSync(args[1], JSON.stringify(out, null, 2));
    console.log(JSON.stringify(out, null, 2));
  } finally { c.close(); }
} else {
  const option = (name, fallback) => { const i = args.indexOf(name); return i < 0 ? fallback : args[i + 1]; };
  const scenario = option('--scenario', 'all');
  assert(['all', 'mouse', 'keyboard', 'normal'].includes(scenario), '未知 scenario');
  const candidates = [process.env.CARGO_TARGET_DIR, join(repo, 'rust/target'), join(repo, '../rust/target')].filter(Boolean).flatMap(t => ['release', 'debug'].map(p => join(t, p, 'jp-app.exe')));
  const newest = candidates.filter(existsSync).sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
  const explicitExe = option('--exe', null);
  const targetDir = resolve(process.env.CARGO_TARGET_DIR ?? (newest ? dirname(dirname(newest)) : join(repo, 'rust/target')));
  const exe = resolve(explicitExe ?? join(targetDir, 'debug/jp-app.exe'));
  const out = resolve(option('--out', join(repo, 'output', `smoke-${Date.now()}`)));
  mkdirSync(out, { recursive: true });
  const report = { exe, started: new Date().toISOString(), scenarios: [], success: false };
  const scenarios = scenario === 'all' ? ['mouse', 'keyboard', 'normal'] : [scenario];
  try {
    if (!explicitExe) {
      // cargo test 会把同名 debug 二进制改成 devUrl 模式；默认命令每次确认前端被嵌入。
      const log = openSync(join(out, 'build.log'), 'w');
      try {
        const ui = spawnSync(process.env.ComSpec ?? 'C:/Windows/System32/cmd.exe', ['/d', '/s', '/c', 'npm run build'], { cwd: join(repo, 'app'), windowsHide: true, stdio: ['ignore', log, log] });
        assert.equal(ui.status, 0, `前端构建失败：${ui.error?.message ?? '查看 build.log'}`);
        const app = spawnSync('cargo', ['build', '-p', 'jp-app', '--features', 'custom-protocol', '--locked', '--target-dir', targetDir], { cwd: join(repo, 'rust'), windowsHide: true, stdio: ['ignore', log, log] });
        assert.equal(app.status, 0, `应用构建失败：${app.error?.message ?? '查看 build.log'}`);
      } finally { closeSync(log); }
      report.built = true;
    }
    assert(existsSync(exe), `找不到应用：${exe}`);
    for (const name of scenarios) {
      const root = mkdtempSync(join(tmpdir(), 'jpop-smoke-'));
      for (const dir of ['bin', 'cwd', 'local', 'webview', 'audio']) mkdirSync(join(root, dir));
      const binary = join(root, 'bin/jp-app.exe'); copyFileSync(exe, binary);
      // 自造音频/歌词，不读取作者曲库；20 秒留给播放状态断言足够时间。
      const sampleRate = 8000, count = sampleRate * 20;
      const wav = Buffer.alloc(44 + count * 2);
      wav.write('RIFF'); wav.writeUInt32LE(wav.length - 8, 4); wav.write('WAVEfmt ', 8);
      wav.writeUInt32LE(16, 16); wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22);
      wav.writeUInt32LE(sampleRate, 24); wav.writeUInt32LE(sampleRate * 2, 28); wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34);
      wav.write('data', 36); wav.writeUInt32LE(count * 2, 40);
      for (let i = 0; i < count; i++) wav.writeInt16LE(Math.round(800 * Math.sin(i * Math.PI * 2 * 440 / sampleRate)), 44 + i * 2);
      writeFileSync(join(root, 'audio/Fixture Artist - Fixture Song.wav'), wav);
      writeFileSync(join(root, 'audio/Fixture Artist - Fixture Song.lrc'), '[00:00.00]テストの空\n[00:02.00]風のテスト\n');
      const server = createServer(); await new Promise(r => server.listen(0, '127.0.0.1', r));
      const port = server.address().port; await new Promise(r => server.close(r));
      const env = { ...process.env, LOCALAPPDATA: join(root, 'local'), WEBVIEW2_USER_DATA_FOLDER: join(root, 'webview'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --lang=zh-CN` };
      delete env.JPOP_CORPUS_HOME; delete env.JPOP_E2E;
      if (name !== 'normal') env.JPOP_E2E = '1';
      const log = openSync(join(out, `${name}.log`), 'w');
      const child = spawn(binary, [], { cwd: join(root, 'cwd'), env, windowsHide: true, stdio: ['ignore', log, log] });
      closeSync(log);
      const result = { name, root, port, pid: child.pid, screenshots: [] }; report.scenarios.push(result);
      let c;
      const screenshot = async phase => {
        const file = join(out, `${name}-${phase}.png`);
        const shot = await c.send('Page.captureScreenshot', { format: 'png' });
        writeFileSync(file, Buffer.from(shot.data, 'base64')); result.screenshots.push(file);
      };
      try {
        c = await connect(port);
        await c.send('Network.enable');
        await c.send('Network.setBlockedURLs', { urls: ['https://*'] });
        await until('应用就绪', () => c.run("!!document.querySelector('.app[data-route]') && !!window.__TAURI_INTERNALS__"));
        // 本用例不检查线上更新；阻断启动计时器的远端查询，避免未来正式版弹窗抢键盘焦点。
        await c.run(`(()=>{const host=window.__TAURI_INTERNALS__;const invoke=host.invoke;host.invoke=(cmd,...args)=>cmd==='update_check'?Promise.reject('smoke: update check disabled'):invoke.call(host,cmd,...args)})()`);
        if (await c.run("!!document.querySelector('.lang-dialog')")) {
          await c.run("[...document.querySelectorAll('.lang-option')].find(b=>b.textContent.includes('中文')).click()");
          await c.run("document.querySelector('.lang-go').click()");
          await until('首次语言选择完成', () => c.run("!!document.querySelector('.app[data-route]') && !document.querySelector('.lang-dialog')"));
        }
        const health = await c.invoke('health'); result.initialHealth = health;
        assert.equal(health.tracks, 0, '必须从空库开始');
        assert(String(health.dbPath).toLowerCase().startsWith(root.toLowerCase()), `曲库未隔离：${health.dbPath}`);
        await screenshot('start');
        if (name === 'normal') {
          assert.equal(await c.run("typeof window.__JPOP_E2E__"), 'undefined');
          for (const cmd of ['e2e_state', 'e2e_scan_folder']) {
            const denied = await c.run(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)},{path:'unused'}).then(()=>false,()=>true)`);
            assert.equal(denied, true, `普通运行不应注册 ${cmd}`);
          }
          result.hooksAbsent = true;
        } else {
          await until('E2E 入口注册', () => c.run('!!window.__JPOP_E2E__'));
          result.before = await c.run('window.__JPOP_E2E__.e2e_state()');
          const scanning = await c.run(`(()=>{window.__JPOP_E2E__.e2e_scan_folder(${JSON.stringify(join(root, 'audio'))});return window.__JPOP_E2E__.e2e_state()})()`);
          assert.equal(scanning.scanning, true, '提交扫描后必须报告 scanning'); result.scanning = scanning;
          await until('可导入计划', () => c.run("document.querySelector('[data-import-plan]')?.dataset.importPlan==='true' && [...document.querySelectorAll('button')].some(b=>b.title==='写库就在这一步'&&!b.disabled)"));
          await screenshot('plan');
          // Synthetic identities must not query real lyric services.
          await c.run("document.querySelector('.import-lyrics-option input:checked')?.click()");
          if (name === 'keyboard') {
            result.audioReady = health.audioReady;
            await c.tabTo("document.activeElement?.title==='写库就在这一步'");
            await c.key(' ', 'Space', 32, ' ');
          } else await c.run("[...document.querySelectorAll('button')].find(b=>b.title==='写库就在这一步').click()");
          await until('导入按钮让曲数 0→1', () => c.run('window.__JPOP_E2E__.e2e_state().trackCount===1'), 15000);
          result.after = await c.run('window.__JPOP_E2E__.e2e_state()');
          assert.equal((await c.invoke('health')).tracks, 1);
          await screenshot('imported');
          if (name === 'keyboard') {
            assert(health.audioReady, '播放键盘回归需要音频输出设备，不能记作通过');
            const tracks = await c.invoke('list_tracks', { limit: 10 });
            await c.invoke('audio_set_volume', { volume: 0 });
            await c.invoke('audio_load', { songId: tracks[0].id, positionSec: 0, autoplay: false });
            await until('播放按钮启用', () => c.run("!!document.querySelector('.play-btn:not(:disabled)')"));
            await c.tabTo("document.activeElement?.classList.contains('play-btn')");
            const before = await c.invoke('audio_state');
            await c.key(' ', 'Space', 32, ' ');
            const after = await until('空格激活播放按钮', async () => { const state = await c.invoke('audio_state'); return state.playState !== before.playState && state; });
            assert.equal(after.playState, 'playing'); result.playback = { before, after };
            await screenshot('playing'); await c.invoke('audio_pause');
          }
          await c.run("[...document.querySelectorAll('.nav button')].find(b=>b.textContent.trim()==='检索').click()");
          await until('检索页', () => c.run("!!document.querySelector('.kwic-page')"));
          await c.run("[...document.querySelectorAll('.kwic-page .segmented button')].find(b=>b.textContent==='全文').click()");
          await until('全文模式', () => c.run("document.querySelector('.kwic-page input.search')?.placeholder==='歌词原文片段'"));
          await c.run("document.querySelector('.kwic-page input.search').focus()");
          await c.send('Input.insertText', { text: 'テスト' }); await c.key('Enter', 'Enter', 13);
          await until('全文结果节点', () => c.run("document.querySelector('.hit-bar')?.textContent.includes('2 条命中') && document.querySelector('.kwic-body')?.textContent.includes('テスト')"));
          result.search = await c.run("document.querySelector('.hit-bar').textContent"); await screenshot('search');
        }
        await c.run("[...document.querySelectorAll('.nav button')].find(b=>b.textContent.trim()==='设置').click()");
        await until('设置页', () => c.run("[...document.querySelectorAll('button')].some(b=>b.textContent.trim()==='关于')"));
        await c.run("[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='关于').click()");
        const prefs = await c.invoke('update_preferences');
        assert(prefs.buildId, '本地构建编号不能为空');
        await until('关于页构建编号', () => c.run(`document.body.textContent.includes(${JSON.stringify(prefs.buildId)})`));
        result.buildId = prefs.buildId; await screenshot('about'); result.success = true;
      } catch (error) {
        result.error = error.stack;
        if (c) { try { await screenshot('failed'); result.dom = await c.run('document.body.innerText'); } catch {} }
        throw error;
      } finally {
        c?.close();
        // 只终止本脚本启动的进程树；保留隔离目录，失败后仍可复查 DB 和日志。
        if (child.pid) {
          const stopped = spawnSync(join(process.env.SystemRoot ?? 'C:/Windows', 'System32/taskkill.exe'), ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, encoding: 'utf8' });
          result.cleanup = { status: stopped.status, error: stopped.error?.message };
          if (stopped.status !== 0) child.kill();
          child.unref();
        }
      }
    }
    report.success = true;
  } catch (error) { report.error = error.stack; process.exitCode = 1; }
  report.finished = new Date().toISOString();
  writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ success: report.success, report: join(out, 'report.json'), scenarios: report.scenarios.map(s => ({ name: s.name, success: s.success, error: s.error })) }, null, 2));
}
