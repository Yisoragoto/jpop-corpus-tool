import { expect, it } from "vitest";
import { monogram } from "./avatarIdentity";

it("keeps numerical identities distinct and marks multiword names",()=>{
  expect(monogram("100回嘔吐")).toBe("100");
  expect(monogram("John Smith")).toBe("JS");
  expect(monogram("ACAね")).toBe("AC");
  expect(monogram("加藤淳人")).toBe("加藤");
});
