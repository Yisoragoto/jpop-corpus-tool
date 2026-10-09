# Scraper 架构

`scraper/` 包只回答一个问题：**「这个本地文件到底是哪首歌？」**

它刻意不做三件事，以保持解耦：

| 不做 | 谁做 |
|---|---|
| 分词、词频、KWIC | corpus pipeline（`legacy/scripts/02_parse_lrc_tokenize.py`、GiNZA） |
| 弹窗、进度条、列表刷新 | `legacy/dialogs/`、`legacy/gui.py` |
| 除状态表外的数据库读写 | 调用方（导入流程） |

因此整个包可以离线、无 GUI 地测试。

## 流水线

```
本地文件
   │
   ├─ extract.MetadataExtractor      内嵌 tag + 文件名 + 目录 + 时长 + 指纹
   │      ↓  TrackFile
   ├─ query.QueryBuilder             由准到宽生成多条查询
   │      ↓  SearchQuery[]
   ├─ providers.*                    iTunes / MusicBrainz / Deezer
   │      ↓  ScrapeCandidate[]
   ├─ matching.MatchScorer           可解释打分
   │      ↓  MatchBreakdown
   ├─ resolver.MetadataResolver      定阈值、定状态
   │      ↓  ResolvedTrack
   └─ store.ScrapeStore              落库（唯一碰 SQLite 的模块）
```

## 各层职责

| 模块 | 职责 | 关键点 |
|---|---|---|
| `normalize.py` | 曲名/歌手/专辑归一化 | **绝不修改原值**，返回同时带 `original` 和 `normalized` 的对象 |
| `filename.py` | 文件名解析 | `01 - Artist - Title`、`[01]`、`1-05`、`Title - Artist`（靠目录线索区分正反） |
| `extract.py` | 路径 → `TrackFile` | 没有内嵌 tag 的文件也能进来；采集时长 |
| `query.py` | `TrackFile` → 查询阶梯 | 完整 → 去版本 → 主唱 → 仅曲名 → 合作歌手 |
| `providers/` | 数据源适配 | 统一接口 + **分类过的**异常；核心不认识具体数据源 |
| `matching.py` | 打分 | 只对双方都有值的字段加权；每次扣分留下原因 |
| `resolver.py` | 编排与判定 | ≥0.90 自动采纳，≥0.55 交给人确认，以下算未匹配 |
| `store.py` | 持久化 | 状态、重试计数、失败原因、原始元数据、候选快照 |

## 几条不能改的设计约束

**归一化不覆盖原值。** 数据库里 `track_original_metadata` 同时存 `title` 和
`normalized_title`。将来改了归一化规则，可以拿原值整库重跑，不会丢信息。

**只对双方都有值的字段打分。** 时长未知时把它当 0 分会系统性压低所有分数，
逼得阈值只能往下调，最后什么都能匹配上。正确做法是让它退出加权，
把权重按比例分给其余分项——`MatchBreakdown.weights` 记录实际用了哪些。

**曲名无法比较时整条匹配为 0。** 否则 provider 先验会独自撑起分数，
一个字段全空的候选能拿到 0.7 分，直接越过 review 阈值。

**时长压过版本标记。** 时长是客观的，`(TV size)` 只是某个人打的字符串。
两者冲突时信时长，否则一大批正确匹配会被标成「需要确认」。

**失败必须分类。** `except Exception: return None` 让上层分不清
「限流了，等会儿重试」和「这首歌本来就查不到」。见 `status.ErrorType`。

**403 是限流，不是「请求写错了」。** iTunes 被打急了会返回 `403` + 空 body，
既没有 `429` 也没有 `Retry-After`。这些接口都是免鉴权的公开搜索，
403 实际上只可能是限流或地区封锁。实测：把 403 归成不可重试的
`INVALID_RESPONSE` 时，209 首里有 70 首被判成永久失败。

**限流时要停下，不要走完查询阶梯。** 一首歌有 5 级查询，每级还各带 3 次重试。
provider 一旦返回限流，`resolver` 就把它拉黑到本次解析结束——
否则失败率会自我放大（实测从 20% 一路涨到 50%）。

**请求内的退避救不了限流。** `HttpClient` 的指数退避总共只等几秒，
而 iTunes 的限流记忆是分钟级的。实测：对 52 首刚失败的曲子立刻重试，
只救回 33%，耗时反而更长。**重试必须是跨会话的**——
把行标成 `FAILED` + `RATE_LIMIT`，隔几分钟再来，而不是当场重试。
`scrape_state.retry_count` / `last_attempt_at` / `RETRYABLE_ERRORS` 就是为此准备的，
真正的调度归 Scraper Queue（第二阶段）。

**当前的节流参数是暂定值。** `itunes.MIN_INTERVAL = 0.25`（4 req/s）实测
前 50~70 首稳定 100%，之后开始被拒。可持续速率需要在 API 冷却状态下重新标定，
标定工作属于拥有全局配速的队列层。

## 数据库

三张新表，全部是新增，不动 `songs` / `utterances` / `tokens` 任何一列
（唯一例外是给 `songs` 加了可空的 `duration_sec`）：

| 表 | 用途 |
|---|---|
| `scrape_state` | 每个文件的当前状态（一行一文件） |
| `scrape_attempts` | 每次尝试的历史，用于排查反复失败 |
| `track_original_metadata` | 原始 + 归一化元数据 |

迁移：

```bash
python legacy/scripts/migrate_db.py
```

幂等，可反复执行；默认先备份到 `backups/`。

## 加一个新数据源

新建 `scraper/providers/yours.py`，继承 `MetadataProvider`，
只有 `search_tracks` 是必须实现的：

```python
class YourProvider(MetadataProvider):
    name = "yours"          # 会落库进 scrape_state.provider，别随便改
    confidence = 0.9        # 先验，参与打分

    def search_tracks(self, query, limit=25):
        payload = self.http.get_json(...)     # HttpClient 已处理超时/重试/限流
        return [ScrapeCandidate(...) for item in payload]
```

**失败要抛 `ProviderError` 子类，不要吞掉返回空列表**——
「没搜到」和「网断了」对上层是完全不同的两件事。

然后在 `resolver.default_resolver()` 里加进去即可，核心逻辑一行不用改。

## 兼容层

`cover_scraper.py` 保留了原有的全部函数签名和返回结构（候选是 dict，
键为 `track / artist / album / year / genre / artwork_url / thumb_url /
match_score`），`legacy/dialogs/song_manager.py` 一行不改就能继续跑。
新代码请直接用 `scraper` 包。

## 跑测试

```bash
python -m unittest discover -s tests -t .
```

只用标准库，不需要装 pytest（pytest 也能直接收集这些用例）。
全部离线，不发一个网络请求。
