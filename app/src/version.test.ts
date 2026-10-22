import { expect, it } from "vitest";
import pkg from "../package.json";
import config from "../../rust/crates/jp-app/tauri.conf.json";
import cargo from "../../rust/crates/jp-app/Cargo.toml?raw";

it("界面、Rust 和安装包的版本一致", () => {
  const version = /^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];
  const actual = `package.json=${pkg.version}, Cargo.toml=${version}, tauri.conf.json=${config.version}`;
  expect(version, actual).toBe(pkg.version);
  expect(config.version, actual).toBe(pkg.version);
  // 安装包用该版本拼文件名；路径分隔符、模板占位符不能混进版本成为另一条路径。
  expect(config.version, actual).toMatch(/^\d+\.\d+\.\d+(?:-[\da-zA-Z.-]+)?(?:\+[\da-zA-Z.-]+)?$/);
});
