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
      if (path.includes("/node_modules/")) return "vendor";
      if (path.includes("/src/locales/")) return "locales";
    } } },
  },
});
