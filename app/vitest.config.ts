import { defineConfig } from "vitest/config";

// 只测纯逻辑（虚拟化数学、二分查找、格式化）。
// 组件测试需要 jsdom + testing-library，那是另一笔投入；
// 现在的价值在于把「算错了不会有人发现」的那部分钉住。
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
