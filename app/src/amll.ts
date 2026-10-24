/**
 * AMLL（打过补丁的副本，见 `app/vendor/amll-core/PATCHES.md`）的入口。
 * 只通过动态 `import("./amll")` 加载：它连着 pixi 和 WebGL 着色器，不进主包，打开全屏歌词或卡拉 OK 时才下载。
 */
import "@applemusic-like-lyrics/core/style.css";

export { BackgroundRender, LyricPlayer, MeshGradientRenderer } from "@applemusic-like-lyrics/core";
export type { LyricLineMouseEvent } from "@applemusic-like-lyrics/core";
