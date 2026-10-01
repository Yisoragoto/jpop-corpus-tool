/*
 * 结构化内容渲染。
 *
 * Yomitan `ext/js/display/structured-content-generator.js` 的 React 移植
 * （Copyright (C) 2023-2026 Yomitan Authors，GPL-3.0-or-later）。
 * 允许的元素、类名、`data-sc-*` 属性、允许的样式属性都和 Yomitan 一致——
 * 词典自带的 styles.css 是照着 Yomitan 生成的 DOM 写的，差一个类名样式就对不上。
 *
 * 和 Yomitan 不同的地方：
 * - 图片走 `dict_media` 取字节建 Blob URL（Yomitan 在 worker 里画 canvas）；
 * - 导入时量不出尺寸的图片（记为 0）按图片加载后的实际大小算宽高比（Yomitan 默认 100×100）；
 * - 站外链接不打开（桌面应用里没有浏览器标签页），站内查词链接（`?query=…`）改成在本应用里查。
 */

import { createElement, Fragment, useEffect, useState, type CSSProperties, type ReactNode } from "react";

import type { StructuredContent, StructuredElement } from "./api";
import { loadMediaUrl, mediaTypeFromPath } from "./media";

export interface RenderContext {
  dictionary: string;
  /** 点了词典里的站内查词链接 */
  onLookup?: ((text: string) => void) | undefined;
}

export function StructuredContentView({ content, context }: { content: StructuredContent; context: RenderContext }) {
  return <span className="structured-content">{renderContent(content, context, "r")}</span>;
}

function renderContent(content: StructuredContent | undefined, context: RenderContext, key: string): ReactNode {
  if (typeof content === "string") {
    return content.length > 0 ? content : null;
  }
  if (typeof content !== "object" || content === null) {
    return null;
  }
  if (Array.isArray(content)) {
    return content.map((item, i) => <Fragment key={`${key}.${i}`}>{renderContent(item, context, `${key}.${i}`)}</Fragment>);
  }
  return renderElement(content, context, key);
}

function renderElement(element: StructuredElement, context: RenderContext, key: string): ReactNode {
  const { tag } = element;
  switch (tag) {
    case "br":
      return simpleElement(tag, element, context, key, false, false);
    case "ruby":
    case "rt":
    case "rp":
      return simpleElement(tag, element, context, key, true, false);
    case "table":
      return (
        <div key={key} className="gloss-sc-table-container">
          {simpleElement(tag, element, context, `${key}.t`, true, false)}
        </div>
      );
    case "thead":
    case "tbody":
    case "tfoot":
    case "tr":
      return simpleElement(tag, element, context, key, true, false);
    case "th":
    case "td":
      return simpleElement(tag, element, context, key, true, true, true);
    case "div":
    case "span":
    case "ol":
    case "ul":
    case "li":
    case "details":
    case "summary":
      return simpleElement(tag, element, context, key, true, true);
    case "img":
      return <DefinitionImage key={key} data={element} dictionary={context.dictionary} />;
    case "a":
      return linkElement(element, context, key);
    default:
      return null;
  }
}

function simpleElement(
  tag: string,
  element: StructuredElement,
  context: RenderContext,
  key: string,
  hasChildren: boolean,
  hasStyle: boolean,
  isCell = false,
): ReactNode {
  const props: Record<string, unknown> = { key, className: `gloss-sc-${tag}` };
  if (typeof element.data === "object" && element.data !== null) {
    Object.assign(props, dataAttributes(element.data));
  }
  if (typeof element.lang === "string") {
    props.lang = element.lang;
  }
  if (isCell) {
    if (typeof element.colSpan === "number") props.colSpan = element.colSpan;
    if (typeof element.rowSpan === "number") props.rowSpan = element.rowSpan;
  }
  if (hasStyle) {
    if (typeof element.style === "object" && element.style !== null) {
      props.style = toStyle(element.style);
    }
    if (typeof element.title === "string") props.title = element.title;
    if (element.open === true) props.open = true;
  }
  return createElement(tag, props, hasChildren ? renderContent(element.content, context, key) : undefined);
}

/** JS `_setElementDataset`：`{content: "sense"}` → `data-sc-content="sense"` */
function dataAttributes(data: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(data)) {
    const name = key.length > 0 ? `${key.charAt(0).toUpperCase()}${key.substring(1)}` : key;
    const attribute = `data-sc${name.replace(/[A-Z]/g, (m) => `-${m.toLowerCase()}`)}`;
    out[attribute] = String(value);
  }
  return out;
}

const STRING_STYLES = [
  "fontStyle",
  "fontWeight",
  "fontSize",
  "color",
  "background",
  "backgroundColor",
  "verticalAlign",
  "textAlign",
  "textEmphasis",
  "textShadow",
  "textDecorationStyle",
  "textDecorationColor",
  "borderColor",
  "borderStyle",
  "borderRadius",
  "borderWidth",
  "clipPath",
  "margin",
  "padding",
  "paddingTop",
  "paddingLeft",
  "paddingRight",
  "paddingBottom",
  "wordBreak",
  "whiteSpace",
  "cursor",
  "listStyleType",
] as const;

const MARGIN_STYLES = ["marginTop", "marginLeft", "marginRight", "marginBottom"] as const;

/** JS `_setStructuredContentElementStyle`：只认这些属性，类型不对的丢掉 */
function toStyle(source: Record<string, unknown>): CSSProperties {
  const style: Record<string, string> = {};
  for (const name of STRING_STYLES) {
    const value = source[name];
    if (typeof value === "string") style[name] = value;
  }
  const decoration = source.textDecorationLine;
  if (typeof decoration === "string") {
    style.textDecoration = decoration;
  } else if (Array.isArray(decoration)) {
    style.textDecoration = decoration.join(" ");
  }
  for (const name of MARGIN_STYLES) {
    const value = source[name];
    if (typeof value === "number") style[name] = `${value}em`;
    if (typeof value === "string") style[name] = value;
  }
  return style as CSSProperties;
}

function linkElement(element: StructuredElement, context: RenderContext, key: string): ReactNode {
  const href = typeof element.href === "string" ? element.href : "";
  const internal = href.startsWith("?");
  const query = internal ? new URLSearchParams(href.substring(1)).get("query") : null;
  return (
    <a
      key={key}
      className="gloss-link"
      data-external={String(!internal)}
      lang={typeof element.lang === "string" ? element.lang : undefined}
      title={internal ? undefined : href}
      href={internal ? "#" : undefined}
      onClick={(event) => {
        event.preventDefault();
        if (query !== null && query.length > 0) context.onLookup?.(query);
      }}
    >
      <span className="gloss-link-text">{renderContent(element.content, context, key)}</span>
      {!internal && <span className="gloss-link-external-icon icon" data-icon="external-link" />}
    </a>
  );
}

/** 图片需要的属性：结构化内容里的 `img` 元素和释义里的 `{type: "image"}` 都满足。 */
export type ImageData = Omit<StructuredElement, "tag" | "content">;

/** JS `createDefinitionImage` */
export function DefinitionImage({ data, dictionary }: { data: ImageData; dictionary: string }) {
  const path = data.path ?? "";
  const [url, setUrl] = useState<string | null>(null);
  const [loadState, setLoadState] = useState<"not-loaded" | "loaded" | "load-error">("not-loaded");
  const [natural, setNatural] = useState<[number, number] | null>(null);

  useEffect(() => {
    let alive = true;
    setUrl(null);
    setNatural(null);
    setLoadState("not-loaded");
    loadMediaUrl(dictionary, path).then(
      (u) => {
        if (alive) setUrl(u);
      },
      () => {
        if (alive) setLoadState("load-error");
      },
    );
    return () => {
      alive = false;
    };
  }, [dictionary, path]);

  const width = typeof data.width === "number" && data.width > 0 ? data.width : (natural?.[0] ?? 100);
  const height = typeof data.height === "number" && data.height > 0 ? data.height : (natural?.[1] ?? 100);
  const preferredWidth = typeof data.preferredWidth === "number" ? data.preferredWidth : null;
  const preferredHeight = typeof data.preferredHeight === "number" ? data.preferredHeight : null;
  const invAspectRatio =
    preferredWidth !== null && preferredHeight !== null ? preferredHeight / preferredWidth : height / width;
  const usedWidth =
    preferredWidth !== null ? preferredWidth : preferredHeight !== null ? preferredHeight / invAspectRatio : width;
  const imageRendering =
    typeof data.imageRendering === "string" ? data.imageRendering : data.pixelated === true ? "pixelated" : "auto";

  return (
    <a
      className="gloss-image-link"
      data-path={path}
      data-dictionary={dictionary}
      data-vector={mediaTypeFromPath(path) === "image/svg+xml" ? "true" : undefined}
      data-image-load-state={loadState}
      data-has-image={url !== null ? "true" : undefined}
      data-has-aspect-ratio="true"
      data-image-rendering={imageRendering}
      data-appearance={typeof data.appearance === "string" ? data.appearance : "auto"}
      data-background={typeof data.background === "boolean" ? String(data.background) : "true"}
      data-collapsed={typeof data.collapsed === "boolean" ? String(data.collapsed) : "false"}
      data-collapsible={typeof data.collapsible === "boolean" ? String(data.collapsible) : "true"}
      data-vertical-align={typeof data.verticalAlign === "string" ? data.verticalAlign : undefined}
      data-size-units={
        typeof data.sizeUnits === "string" && (preferredWidth !== null || preferredHeight !== null)
          ? data.sizeUnits
          : undefined
      }
      onClick={(event) => event.preventDefault()}
    >
      <span
        className="gloss-image-container"
        style={{
          width: `${usedWidth}em`,
          ...(typeof data.border === "string" ? { border: data.border } : {}),
          ...(typeof data.borderRadius === "string" ? { borderRadius: data.borderRadius } : {}),
        }}
        title={typeof data.title === "string" ? data.title : undefined}
      >
        <span className="gloss-image-sizer" style={{ paddingTop: `${invAspectRatio * 100}%` }} />
        <span
          className="gloss-image-background"
          style={url !== null ? ({ "--image": `url("${url}")` } as CSSProperties) : undefined}
        />
        <span className="gloss-image-container-overlay" />
        {url !== null && (
          <img
            className="gloss-image"
            src={url}
            alt={typeof data.alt === "string" ? data.alt : ""}
            onLoad={(event) => {
              setLoadState("loaded");
              const img = event.currentTarget;
              if (img.naturalWidth > 0 && img.naturalHeight > 0) {
                setNatural([img.naturalWidth, img.naturalHeight]);
              }
            }}
            onError={() => setLoadState("load-error")}
          />
        )}
      </span>
      <span className="gloss-image-link-text">Image</span>
    </a>
  );
}
