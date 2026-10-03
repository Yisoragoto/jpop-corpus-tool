/**
 * 设置 → 系统 → 变调缓存。
 *
 * 变调是把整首歌渲染成一份 WAV 存下来再播（一首 4 分钟的歌约 45 MB），放过的调越多占得越多。
 * 这里看得到现在占了多少，也能改上限；超过上限时从最久没放的清起，清掉的下次要用再渲染。
 *
 * 上限和别的偏好一样记在 `settings.ts`；后端只管这次运行里用哪个数，
 * 所以改的时候两边都要说一声（启动时那一声在 `App.tsx`）。
 */

import { useEffect, useState } from "react";

import { api, type PitchCacheStatus } from "../api";
import { PITCH_CACHE_LIMITS_MB, limitLabel, usageLabel } from "../pitchCache";
import { updateSettings, useAppSettings } from "../settings";
import { SettingCard, SettingSelect } from "./SettingCard";

interface Props {
  icon: React.ReactNode;
  onError: (message: string) => void;
}

export function PitchCacheCard({ icon, onError }: Props) {
  const settings = useAppSettings();
  const [status, setStatus] = useState<PitchCacheStatus | null>(null);

  useEffect(() => {
    void api
      .pitchCacheStatus()
      .then(setStatus)
      .catch(() => undefined);
  }, []);

  // 手改过存储、存了一个不在列表里的数：照样显示出来，不悄悄换成别的
  const listed: readonly number[] = PITCH_CACHE_LIMITS_MB;
  const limits = listed.includes(settings.pitchCacheLimitMb)
    ? [...listed]
    : [...listed, settings.pitchCacheLimitMb].sort((a, b) => a - b);

  return (
    <SettingCard
      icon={icon}
      title="变调缓存"
      description="变调过的歌存一份在语料库里（一首约 45 MB）。超过上限时清掉最久没放的，下次用到再重新生成"
    >
      <span className="muted small">{status === null ? "" : usageLabel(status)}</span>
      <SettingSelect
        value={String(settings.pitchCacheLimitMb)}
        options={limits.map((mb) => ({ value: String(mb), label: `上限 ${limitLabel(mb)}` }))}
        onChange={(value) => {
          const limitMb = Number(value);
          updateSettings({ pitchCacheLimitMb: limitMb });
          // 调小的话后端会立刻清一遍，返回的是清完之后的占用
          api
            .pitchCacheSetLimit(limitMb)
            .then(setStatus)
            .catch((err: unknown) => onError(err instanceof Error ? err.message : String(err)));
        }}
      />
    </SettingCard>
  );
}
