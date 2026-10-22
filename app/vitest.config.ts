import { defineConfig } from "vitest/config";

// 默认运行纯逻辑与源码守卫；歌词单位组件通过文件级注解使用 happy-dom。
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
    // token 守卫要读取原始 CSS；默认的空串 mock 会让新增硬编码颜色也假绿。
    css: true,
  },
});
