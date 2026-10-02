// 连上开着远程调试端口的 WebView2，调几个命令，把结果打成 JSON。
// 只给 release-check.ps1 用；手头没有别的依赖，node 20 自带 fetch 和 WebSocket。
//
//   node scripts/smoke.mjs <调试端口>
//
// 退出码：0 一切正常，1 连不上或命令失败。

const port = process.argv[2];
if (!port) {
  console.error('用法：node scripts/smoke.mjs <调试端口>');
  process.exit(1);
}

/** 等 WebView2 把调试端口打开——应用启动要几秒 */
async function target(deadlineMs = 40000) {
  const until = Date.now() + deadlineMs;
  let last;
  while (Date.now() < until) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = list.find((t) => t.type === 'page' && t.url.startsWith('http'));
      if (page) return page;
      last = `还没有页面：${JSON.stringify(list.map((t) => t.url))}`;
    } catch (err) {
      last = String(err);
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error(`连不上调试端口 ${port}：${last}`);
}

const page = await target();
const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();
ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
};
await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = () => reject(new Error('WebSocket 打不开'));
});

function send(method, params) {
  const id = ++nextId;
  return new Promise((resolve) => {
    pending.set(id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });
}

/** 在页面里跑一段 async 代码，取返回值 */
async function run(expression) {
  const res = await send('Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  const result = res.result?.result;
  if (res.result?.exceptionDetails) {
    throw new Error(res.result.exceptionDetails.exception?.description ?? '页面里报错');
  }
  return result?.value;
}

// 界面渲染出来没有、后端活着没有：health 是最短的那条「从前端一路问到 SQLite」
const out = await run(`(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const health = await invoke('health', {});
  const dict = await invoke('tokenizer_status', {});
  return {
    href: location.href,
    navButtons: [...document.querySelectorAll('.nav button')].map((b) => b.textContent.trim()),
    health,
    dict: { ready: dict.ready, source: dict.source },
  };
})()`);

console.log(JSON.stringify(out, null, 1));
ws.close();
process.exit(0);
