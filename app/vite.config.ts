import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 会在 devUrl 上加载这个 dev server；端口必须和 tauri.conf.json 对上。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: {
    target: "chrome110", sourcemap: true,
    rollupOptions: { output: { manualChunks(id) {
      // Keep the growing translation catalog and React out of the main bundle.
      // Smaller chunks also avoid esbuild's large-input temporary-file path on Windows.
      const path = id.replaceAll("\\", "/");
      // 启动就要的第三方只有 React 和 Tauri；其余第三方包（pixi 和它的依赖、gl-matrix……）都是 AMLL 带进来的，
      // 只在全屏歌词 / 卡拉 OK 里动态加载，单独一块。以后加了启动就要用的依赖，记得加到这个名单里
      if (/\/node_modules\/(react|react-dom|scheduler|@tauri-apps)\//.test(path)) return "vendor";
      if (path.includes("/node_modules/") || path.includes("/vendor/amll-core/") || path.endsWith("/src/amll.ts")) return "amll";
      if (path.includes("/src/locales/")) return "locales";
    } } },
  },
});
