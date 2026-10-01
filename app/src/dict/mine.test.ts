import { describe, expect, it } from "vitest";

import { artistSubdeck } from "./mine";

// 和 rust/crates/jp-anki/src/mine.rs 的 subdeck_names_take_the_first_performer_and_never_add_a_level 同一组例子
describe("按歌手放的子牌组名", () => {
  it("合作曲取第一位歌手，不多出一层，空的不放", () => {
    expect(artistSubdeck("JPOP", "ヨルシカ")).toBe("JPOP::ヨルシカ");
    expect(artistSubdeck("JPOP", "ずっと真夜中でいいのに。/森カリオペ")).toBe("JPOP::ずっと真夜中でいいのに。");
    expect(artistSubdeck("日本語::歌詞", " Mrs. GREEN APPLE ")).toBe("日本語::歌詞::Mrs. GREEN APPLE");
    expect(artistSubdeck("JPOP", "A::B:::C")).toBe("JPOP::A:B:C");
    expect(artistSubdeck("JPOP", " / feat")).toBeNull();
    expect(artistSubdeck("", "ヨルシカ")).toBeNull();
  });
});
