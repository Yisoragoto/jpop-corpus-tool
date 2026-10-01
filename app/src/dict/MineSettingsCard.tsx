/**
 * 「一键制卡」的设置：牌组、歌手子牌组、笔记类型、首选释义词典、卡片类型。
 * 改了立即生效（曲库页的制卡按钮读同一份设置）。
 *
 * 设置页和 Anki 页共用这一份，所以做成设置行（`SettingCard`）而不是某一页专有的版面。
 */

import { useEffect, useState } from "react";

import { SettingCard, SettingGroup, SettingNote, SettingSelect, Switch } from "../components/SettingCard";
import { dictApi, type DictionaryInfo } from "./api";
import {
  LAPIS_CARD_KIND_LABELS,
  LYRICS_MODEL,
  mineApi,
  updateMineSettings,
  useMineSettings,
  type LapisCardKind,
} from "./mine";

interface Props {
  connected: boolean;
  decks: string[];
  /** 设置页里外面已经有「制卡」这个组标题了，就不用再写一遍 */
  title?: string;
}

export function MineSettingsCard({ connected, decks, title }: Props) {
  const settings = useMineSettings();
  const [models, setModels] = useState<string[]>([]);
  const [dictionaries, setDictionaries] = useState<DictionaryInfo[]>([]);

  useEffect(() => {
    if (connected) void mineApi.modelNames().then(setModels).catch(() => undefined);
  }, [connected]);
  useEffect(() => {
    void dictApi.list().then(setDictionaries).catch(() => undefined);
  }, []);

  const termDictionaries = dictionaries.filter((d) => d.enabled && (d.counts.terms ?? 0) > 0);
  // 选项里至少要有当前值，否则 select 显示成空白
  const deckOptions = (decks.includes(settings.deck) ? decks : [settings.deck, ...decks]).map((d) => ({ value: d, label: d }));
  // Lyrics 是应用自己的笔记类型，Anki 里还没有也列出来（第一次制卡时建）
  const modelOptions = [...new Set([settings.model, LYRICS_MODEL, ...models])].map((m) => ({ value: m, label: m }));
  const lyricsMissing = connected && models.length > 0 && !models.includes(LYRICS_MODEL);

  const dictionaryOptions = [
    { value: "", label: "排在最前的词典" },
    ...termDictionaries.map((d) => ({ value: d.title, label: d.title })),
  ];
  if (settings.mainDictionary !== null && !termDictionaries.some((d) => d.title === settings.mainDictionary)) {
    dictionaryOptions.push({ value: settings.mainDictionary, label: `${settings.mainDictionary}（未启用）` });
  }

  return (
    <SettingGroup title={title}>
      <SettingCard
        title="牌组"
        description={
          connected
            ? "在曲库里点词，查词结果上的「＋ 制卡」就放进这里"
            : "Anki 没连上，列不出牌组；连上之后这里会是真实的牌组列表"
        }
      >
        <SettingSelect
          value={settings.deck}
          options={deckOptions}
          onChange={(deck) => updateMineSettings({ deck })}
          disabled={!connected}
        />
      </SettingCard>

      <SettingCard
        title="按歌手放进子牌组"
        description={
          settings.artistSubdeck
            ? `放进「${settings.deck}::歌手」，例如「${settings.deck}::ヨルシカ」；子牌组没有就新建，合作曲放进第一位歌手的。不是从歌里查的词还放在上面那个牌组`
            : "开了以后制卡放进「牌组::歌手」的子牌组，没有就新建"
        }
      >
        <Switch
          checked={settings.artistSubdeck}
          onChange={(artistSubdeck) => updateMineSettings({ artistSubdeck })}
          label="按歌手放进子牌组"
        />
      </SettingCard>

      <SettingCard
        title="笔记类型"
        description="默认「Lyrics」：照 Lapis 做的，背面右侧是封面，封面右边竖排歌名、歌手、专辑"
      >
        <SettingSelect value={settings.model} options={modelOptions} onChange={(model) => updateMineSettings({ model })} />
      </SettingCard>

      <SettingCard title="首选释义" description="MainDefinition 用哪本词典；这个词那本里没有时，用排在最前、有释义的那本">
        <SettingSelect
          value={settings.mainDictionary ?? ""}
          options={dictionaryOptions}
          onChange={(value) => updateMineSettings({ mainDictionary: value === "" ? null : value })}
        />
      </SettingCard>

      <SettingCard title="卡片类型" description="例句是点的那一行（查的词加粗），这首歌里其他含这个词的句子接在后面，每句都从歌里切一段音频">
        <SettingSelect
          value={settings.kind}
          options={(Object.keys(LAPIS_CARD_KIND_LABELS) as LapisCardKind[]).map((k) => ({
            value: k,
            label: LAPIS_CARD_KIND_LABELS[k],
          }))}
          onChange={(kind) => updateMineSettings({ kind })}
        />
      </SettingCard>

      {settings.model === LYRICS_MODEL && lyricsMissing && (
        <SettingNote>Anki 里还没有「Lyrics」笔记类型，第一次制卡时会自动建。</SettingNote>
      )}
      {connected && models.length > 0 && settings.model !== LYRICS_MODEL && !models.includes(settings.model) && (
        <SettingNote>
          <span className="warn">Anki 里没有「{settings.model}」笔记类型，换一个。</span>
        </SettingNote>
      )}
      {connected && decks.length > 0 && !decks.includes(settings.deck) && (
        <SettingNote>
          <span className="warn">Anki 里没有「{settings.deck}」牌组，制卡会失败。</span>
        </SettingNote>
      )}
    </SettingGroup>
  );
}
