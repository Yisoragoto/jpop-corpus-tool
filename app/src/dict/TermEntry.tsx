/**
 * 一条查词结果（Yomitan 的「词条」）。
 *
 * 结构参考 Yomitan `ext/js/display/display-generator.js`：
 * 词头（带注音）→ 活用链 → 词头标签 → 词频 → 音高 → 按词典分组的释义。
 * 释义里的结构化内容、图片交给 `StructuredContent`，每组释义带 `data-dictionary`，
 * 词典自带的样式按它限定作用域。
 *
 * 折叠：点词典名折叠这一本（记住，之后查词也折叠，只留一行预览）；
 * 展开时释义太长也先截断，点「展开全文」看全部。
 */

import { Fragment, useLayoutEffect, useRef, useState, type ReactNode } from "react";

import type {
  GlossaryEntry,
  Tag,
  TermDefinition,
  TermDictionaryEntry,
  TermFrequency,
  TermHeadword,
  TermPronunciation,
} from "./api";
import { setDictionaryCollapsed, useCollapsedDictionaries } from "./collapse";
import { previewText, splitLineBreaks } from "./glossaryText";
import { distributeFurigana } from "./japanese";
import { DownstepNotation, PitchText } from "./Pronunciation";
import { DefinitionImage, StructuredContentView, type RenderContext } from "./StructuredContent";

/** 一本词典的释义展开后最多先显示多高（像素），超出截断 */
const CLAMP_HEIGHT = { full: 420, compact: 200 };

interface Props {
  entry: TermDictionaryEntry;
  onLookup?: ((text: string) => void) | undefined;
  /** 曲库右栏这种窄栏：截断得更早 */
  compact?: boolean | undefined;
  /** 词头右边的按钮（比如制卡） */
  actions?: ReactNode;
}

export function TermEntry({ entry, onLookup, compact = false, actions }: Props) {
  const multipleHeadwords = entry.headwords.length > 1;
  return (
    <article className="dict-entry">
      <header className="dict-entry-head">
        <div className="dict-headwords">
          {entry.headwords.map((headword) => (
            <Headword key={headword.index} headword={headword} />
          ))}
        </div>
        <InflectionChains entry={entry} />
        <TagList tags={mergeTags(entry.headwords.flatMap((h) => h.tags))} />
        {actions !== undefined && actions !== null && <div className="dict-entry-actions">{actions}</div>}
      </header>
      <Frequencies frequencies={entry.frequencies} headwords={entry.headwords} showHeadword={multipleHeadwords} />
      <Pronunciations
        pronunciations={entry.pronunciations}
        headwords={entry.headwords}
        showHeadword={multipleHeadwords}
      />
      <Definitions definitions={entry.definitions} headwords={entry.headwords} onLookup={onLookup} compact={compact} />
    </article>
  );
}

function headwordLabel(headword: TermHeadword | undefined): string {
  if (headword === undefined) return "";
  return headword.term === headword.reading ? headword.term : `${headword.term}【${headword.reading}】`;
}

/** 按 key 分组，保持第一次出现的先后。 */
function groupBy<T>(items: T[], key: (item: T) => string): [string, T[]][] {
  const groups: [string, T[]][] = [];
  const index = new Map<string, T[]>();
  for (const item of items) {
    const k = key(item);
    let list = index.get(k);
    if (list === undefined) {
      list = [];
      index.set(k, list);
      groups.push([k, list]);
    }
    list.push(item);
  }
  return groups;
}

/** 多个词头的标签合在一起显示，同名同类只留一个。 */
function mergeTags(tags: Tag[]): Tag[] {
  const seen = new Set<string>();
  return tags.filter((tag) => {
    const key = `${tag.category} ${tag.name}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function Headword({ headword }: { headword: TermHeadword }) {
  const segments = distributeFurigana(headword.term, headword.reading);
  return (
    <span className="dict-headword" lang="ja">
      {segments.map((segment, i) =>
        segment.reading.length > 0 ? (
          <ruby key={i}>
            {segment.text}
            <rt>{segment.reading}</rt>
          </ruby>
        ) : (
          <Fragment key={i}>{segment.text}</Fragment>
        ),
      )}
    </span>
  );
}

const INFLECTION_SOURCE_LABELS: Record<string, string> = {
  algorithm: "",
  dictionary: "词典给出的变形",
  both: "活用规则与词典都认可",
};

function InflectionChains({ entry }: { entry: TermDictionaryEntry }) {
  const chains = entry.inflectionRuleChainCandidates.filter((chain) => chain.inflectionRules.length > 0);
  if (chains.length === 0) return null;
  return (
    <div className="dict-inflections">
      {chains.map((chain, i) => (
        <span
          key={i}
          className="dict-inflection-chain"
          data-source={chain.source}
          title={INFLECTION_SOURCE_LABELS[chain.source] || undefined}
        >
          {chain.inflectionRules.map((rule, j) => (
            <span key={j} className="dict-inflection" title={rule.description}>
              {rule.name}
            </span>
          ))}
        </span>
      ))}
    </div>
  );
}

function TagList({ tags }: { tags: Tag[] }) {
  const visible = tags.filter((tag) => !tag.redundant);
  if (visible.length === 0) return null;
  return (
    <span className="dict-tags">
      {visible.map((tag, i) => (
        <span
          key={`${tag.category}-${tag.name}-${i}`}
          className="dict-tag"
          data-category={tag.category}
          title={[...tag.content, tag.dictionaries.join("、")].filter((s) => s.length > 0).join("\n")}
        >
          {tag.name}
        </span>
      ))}
    </span>
  );
}

function Frequencies({
  frequencies,
  headwords,
  showHeadword,
}: {
  frequencies: TermFrequency[];
  headwords: TermHeadword[];
  showHeadword: boolean;
}) {
  if (frequencies.length === 0) return null;
  return (
    <div className="dict-frequencies">
      {groupBy(frequencies, (f) => f.dictionary).map(([dictionary, items]) => (
        <span
          key={dictionary}
          className="dict-frequency-group"
          data-frequency-mode={items[0]?.frequencyMode ?? undefined}
          title={items[0]?.frequencyMode === "rank-based" ? "按排名：数字越小越常用" : undefined}
        >
          <span className="dict-frequency-dictionary">{items[0]?.dictionaryAlias ?? dictionary}</span>
          <span className="dict-frequency-values">
            {items.map((f, i) => (
              <span key={i} className="dict-frequency-value" title={f.hasReading ? undefined : "不分读音"}>
                {showHeadword && <span className="dict-frequency-headword">{headwordLabel(headwords[f.headwordIndex])} </span>}
                {f.displayValue ?? String(f.frequency)}
              </span>
            ))}
          </span>
        </span>
      ))}
    </div>
  );
}

function Pronunciations({
  pronunciations,
  headwords,
  showHeadword,
}: {
  pronunciations: TermPronunciation[];
  headwords: TermHeadword[];
  showHeadword: boolean;
}) {
  if (pronunciations.every((p) => p.pronunciations.length === 0)) return null;
  return (
    <div className="dict-pronunciations">
      {groupBy(pronunciations, (p) => p.dictionary).map(([dictionary, list]) => (
        <div key={dictionary} className="dict-pronunciation-group">
          <span className="dict-pronunciation-dictionary">{list[0]?.dictionaryAlias ?? dictionary}</span>
          {list.flatMap((p) => {
            const headword = headwords[p.headwordIndex];
            const reading = headword?.reading ?? "";
            return p.pronunciations.map((item, i) => (
              <span key={`${p.index}-${i}`} className="dict-pronunciation" lang="ja">
                {showHeadword && headword !== undefined && (
                  <span className="dict-pronunciation-headword">{headword.term}</span>
                )}
                {item.type === "pitch-accent" ? (
                  <>
                    <PitchText
                      reading={reading}
                      positions={item.positions}
                      nasalPositions={item.nasalPositions}
                      devoicePositions={item.devoicePositions}
                    />
                    <DownstepNotation positions={item.positions} />
                  </>
                ) : (
                  <span className="dict-ipa">{item.ipa}</span>
                )}
                <TagList tags={item.tags} />
              </span>
            ));
          })}
        </div>
      ))}
    </div>
  );
}

/** 内容超过 `maxHeight` 时截断，底部渐隐，给「展开全文」。 */
function Clamp({ maxHeight, children }: { maxHeight: number; children: ReactNode }) {
  const bodyRef = useRef<HTMLDivElement>(null);
  const [overflow, setOverflow] = useState(false);
  const [open, setOpen] = useState(false);

  useLayoutEffect(() => {
    const body = bodyRef.current;
    if (body === null) return;
    // 留一点余量：只超出一两行就不截了，截了反而更难读
    const check = () => setOverflow(body.scrollHeight > maxHeight + 48);
    check();
    // 图片加载完内容会变高
    const observer = new ResizeObserver(check);
    observer.observe(body);
    for (const child of body.children) observer.observe(child);
    return () => observer.disconnect();
  }, [maxHeight, children]);

  const clamped = overflow && !open;
  return (
    <div className="dict-clamp" data-clamped={clamped ? "true" : undefined}>
      <div ref={bodyRef} className="dict-clamp-body" style={clamped ? { maxHeight } : undefined}>
        {children}
      </div>
      {overflow && (
        <button className="dict-clamp-toggle" onClick={() => setOpen(!open)}>
          {open ? "收起" : "展开全文"}
        </button>
      )}
    </div>
  );
}

function Definitions({
  definitions,
  headwords,
  onLookup,
  compact,
}: {
  definitions: TermDefinition[];
  headwords: TermHeadword[];
  onLookup?: ((text: string) => void) | undefined;
  compact: boolean;
}) {
  const collapsed = useCollapsedDictionaries();
  // 释义已按词典顺序排好；连续同一本的归成一组
  const groups: [string, TermDefinition[]][] = [];
  for (const definition of definitions) {
    const last = groups[groups.length - 1];
    if (last !== undefined && last[0] === definition.dictionary) {
      last[1].push(definition);
    } else {
      groups.push([definition.dictionary, [definition]]);
    }
  }
  return (
    <div className="dict-definitions">
      {groups.map(([dictionary, list], gi) => {
        const isCollapsed = collapsed.has(dictionary);
        return (
          <section
            key={`${dictionary}-${gi}`}
            className="dict-dictionary"
            data-collapsed={isCollapsed ? "true" : undefined}
          >
            <button
              className="dict-dictionary-name"
              onClick={() => setDictionaryCollapsed(dictionary, !isCollapsed)}
              title={isCollapsed ? "展开这本词典（之后查词也保持展开）" : "折叠这本词典（之后查词也保持折叠）"}
            >
              <span className="dict-chevron" aria-hidden="true">
                {isCollapsed ? "▸" : "▾"}
              </span>
              {list[0]?.dictionaryAlias ?? dictionary}
            </button>
            {isCollapsed ? (
              <button className="dict-dictionary-preview" onClick={() => setDictionaryCollapsed(dictionary, false)}>
                {previewText(list.flatMap((d) => d.entries))}
              </button>
            ) : (
              <Clamp maxHeight={compact ? CLAMP_HEIGHT.compact : CLAMP_HEIGHT.full}>
                <ol className="dict-definition-list">
                  {list.map((definition) => (
                    <li key={definition.id} className="dict-definition definition-item" data-dictionary={dictionary}>
                      {(headwords.length > 1 && definition.headwordIndices.length < headwords.length) ||
                      definition.tags.some((t) => !t.redundant) ? (
                        <div className="dict-definition-meta">
                          {headwords.length > 1 && definition.headwordIndices.length < headwords.length && (
                            <span className="dict-definition-only">
                              {definition.headwordIndices.map((i) => headwordLabel(headwords[i])).join("、")}
                            </span>
                          )}
                          <TagList tags={definition.tags} />
                        </div>
                      ) : null}
                      <ul className="gloss-list">
                        {definition.entries.map((glossary, i) => (
                          <li key={i} className="gloss-item">
                            <Glossary entry={glossary} context={{ dictionary, onLookup }} />
                          </li>
                        ))}
                      </ul>
                    </li>
                  ))}
                </ol>
              </Clamp>
            )}
          </section>
        );
      })}
    </div>
  );
}

function Glossary({ entry, context }: { entry: GlossaryEntry; context: RenderContext }) {
  if (typeof entry === "string") {
    return (
      <span className="gloss-text">
        {splitLineBreaks(entry).map((line, i) => (
          <Fragment key={i}>
            {i > 0 && <br />}
            {line}
          </Fragment>
        ))}
      </span>
    );
  }
  if (entry.type === "structured-content") {
    return <StructuredContentView content={entry.content} context={context} />;
  }
  return (
    <>
      <DefinitionImage data={entry} dictionary={context.dictionary} />
      {typeof entry.description === "string" && <span className="gloss-image-description">{entry.description}</span>}
    </>
  );
}
