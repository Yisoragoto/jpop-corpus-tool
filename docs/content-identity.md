# 内容身份与来源

结构版本 20261020 只加列、表和索引：歌曲有 ISRC、录音 MBID、音频指纹；专辑有发行组 MBID、
厂牌、条码、类型、曲数；`external_ids` 保存带来源和置信度的外部身份。
实体字段与 `field_provenance`、external_ids 在同一保存点写入，人工字段不覆盖，空值才补。
候选专辑与本地专辑名称不匹配时不写专辑身份，避免把合辑的厂牌安到原专辑上。
删歌或删掉孤立专辑时清理对应身份和字段来源，防止歌曲数字 id 复用后继承旧身份。

MusicBrainz 搜索不支持 inc。按[官方 API](https://musicbrainz.org/doc/MusicBrainz_API) 的发行详情接口，
候选确定后一次请求包含 recordings、isrcs、release-groups、labels、artist-credits、media，
按录音 MBID 从介质曲目里挑对应 ISRC；不会取同一专辑另一首歌的 ISRC。
见[搜索参数说明](https://musicbrainz.org/doc/MusicBrainz_API/Search)。这与原计划“直接扩展搜索 inc”不同，
是按接口实际能力作出的调整，没有按字段逐个请求。

Deezer 从曲目详情取 ISRC，从专辑详情同时取厂牌、曲数、完整日期、类型、条码；搜索响应若已有就直接复用。
它放在 iTunes、MusicBrainz 之后做曲目候选的兜底。详情失败不伪造身份，警告保留在候选的 identity_warning。
无法确认的候选仍停在人工复核，不自动写库。

音频指纹规则 `pcm8k-mono-i16le-30s-v1`：解码前 30 秒，声道平均成单声道，线性重采样至 8 kHz，
量化为 PCM16 小端字节，SHA-256。只用已在依赖树中的 rodio 解码特性和 sha2，不需要输出设备，不改播放算法。
这是同文件身份：改名、移动不影响它。不是跨压制识别，也不能保证共享前奏的不同歌一定不同；本期不据此自动合并歌曲。

导入和重新链接音频时写指纹；不可解码时留空并输出原因，不阻断既有歌词导入。
`jp fingerprint` 显式重扫已导入歌曲并写库（JSON 报告含成功数和错误）；`jp scan` 继续只读。
不在只读扫描中偷偷升级或回写用户库。
