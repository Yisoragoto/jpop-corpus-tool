/**
 * AMLL 的流动封面背景（MeshGradientRenderer，WebGL）。全屏歌词和卡拉 OK 共用。
 *
 * 封面直接给 asset 地址：AMLL 自己 `fetch` 成 blob 再当图片读（asset 协议的响应带 CORS 头，CSP 的 connect-src 放行了 asset，
 * img-src 放行了 blob:），WebGL 读纹理不会被当成跨域污染。**别先 fetch 成 blob URL 再给它**：它会再 fetch 一次，
 * 而 connect-src 不放行 blob:。暂停时背景也停，省显卡。WebGL 起不来时调 `onFail`，调用方退回原来的背景。
 */

import { useEffect, useRef } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

interface Props {
  coverPath: string;
  playing: boolean;
  onFail: () => void;
}

type Bg = { setAlbum: (src: string) => Promise<void>; pause: () => void; resume: () => void; dispose: () => void; getElement: () => HTMLElement };

export function AmllBackground({ coverPath, playing, onFail }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const bg = useRef<Bg | null>(null);
  const failed = useRef(onFail);
  failed.current = onFail;
  const playingNow = useRef(playing);
  playingNow.current = playing;

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const amll = await import("../amll");
        if (!alive || box.current === null) return;
        const render = amll.BackgroundRender.new(amll.MeshGradientRenderer) as unknown as Bg;
        bg.current = render;
        const el = render.getElement();
        el.style.width = "100%";
        el.style.height = "100%";
        box.current.appendChild(el);
        await render.setAlbum(convertFileSrc(coverPath));
        if (!playingNow.current) render.pause();
      } catch {
        if (alive) failed.current();
      }
    })();
    return () => {
      alive = false;
      bg.current?.dispose();
      bg.current = null;
    };
  }, [coverPath]);

  useEffect(() => {
    if (playing) bg.current?.resume();
    else bg.current?.pause();
  }, [playing]);

  return <div className="amll-bg" ref={box} aria-hidden="true" />;
}
