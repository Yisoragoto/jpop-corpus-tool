import { defineConfig } from "vitest/config";

// 只测纯逻辑（虚拟化数学、二分查找、格式化）。
// 组件测试需要 jsdom + testing-library，那是另一笔投入；
// 现在的价值在于把「算错了不会有人发现」的那部分钉住。
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
    // token 守卫要读取原始 CSS；默认的空串 mock 会让新增硬编码颜色也假绿。
    css: true,
  },
});
