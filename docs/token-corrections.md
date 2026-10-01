# 分词校正

在 KWIC 里右键一行，把分错的词合并、拆开，或者改词元和词性。
对应 `rust/crates/jp-corpus/src/corrections.rs`，Tauri 入口是 `token_correction` /
`save_token_correction` / `revert_token_correction`，界面是 `TokenCorrectionDialog`。

## 和 Python 版共用一张表

`token_corrections` 的表结构和 `scripts/migrate_db.py::_ensure_token_corrections` 逐字一致，
JSON 形状和 `gui.py::_apply_token_correction` 一致：

```json
[{"surface": "夜が", "lemma": "夜", "pos": "NOUN"}, {"surface": "明ける", "lemma": "明ける", "pos": "VERB"}]
```

读的时候 `lemma` 可以缺省（Python 是 `t.get('lemma', t['surface'])`）。
表不存在是正常状态——从没校正过的库不需要它，第一次保存时才建。

## 保存

1. 清理：去首尾空白，丢掉空词，词元缺省用表层形，词性缺省 `NOUN`（和 Python 编辑器的 `_current_tokens` 一样）
2. `INSERT OR REPLACE` 校正行，带上歌词原文、歌手、曲名——恢复时靠这三个键
3. 删掉这一行的 `tokens`，写入校正后的词

**原始分词只在第一次校正时记下。** 之后再改，「原始」仍是分词器给的那份，
撤销永远回到分词器的结果，而不是上一次手改的结果。

## 删歌再导入时恢复

删歌不删校正（`dialogs/song_manager.py::_delete` 只删 utterances、tokens、songs），
校正行留下来，`utterance_id` 指向已经不存在的行。重新导入同一首歌时（`jp-import` 的 `import_one`，
歌词和分词写完之后、同一个事务里）：

1. 按 `(歌手, 曲名)` 取出校正
2. 逐行比对文本：先精确匹配，再用 NFKC + 去首尾空白兜底
3. 对上了就把校正套回去，校正行改指向新的 `utterance_id`

导入报告里「分词校正已恢复」一栏显示套回去的行数。没分词（找不到词典）时不套，
只给个别行写 token 会得到一首「半分词」的歌；校正行留在表里不会丢。

和 Python 一致的细节，都有测试钉着：

| 情况 | 行为 |
|---|---|
| 同一句有好几条校正 | 后面的覆盖前面的（按 `(text, utterance_id)` 排序，和 Python dict 推导式走索引时的顺序一样） |
| 副歌重复 | 每一行都套上校正，但校正行只挪到第一次出现的那行 |
| 另一首歌有同样的歌词 | 不会串过去——键里有歌手和曲名 |
| 已经指向这一行 | 跳过，重复调用不会重复计数 |

## 跨工具验证（真实数据）

两个版本共用一个库，真正会发生的场景是：**在 PyQt 里校正、在 PyQt 里删歌、在 Tauri 里重新导入。**
单元测试只能证明 Rust 自己存、自己读是对的，所以另外在库的临时副本上跑了一遍：

1. 调 `gui.py` 自己的 `_apply_token_correction` 存校正（真 Python 代码，不是仿写）
2. 用 `song_manager.py::_delete` 里一模一样的四条 SQL 删歌
3. 用 `cargo run --release -p jp-import --example reimport_song -- <副本> <音频>` 重新导入
4. 照抄 `token_correction.py::_load_tokens` 的读法读回来

サカナクション《さよならはエモーション》第 1 行「深夜のコンビニエンスストア」：

| 步骤 | 结果 |
|---|---|
| Python 校正 | `深夜 / の / コンビニエンスストア` → `深夜の / コンビニエンスストア` |
| 删歌后 | 校正行留下 1 条 |
| Rust 重导 | 计划新增 1，导入 1，失败 0，**恢复 1 行**；新歌 38 行里只有这 1 行文本相同 |
| 分词 | 新行的 tokens 等于校正；其余行没被套上 |
| 校正行 | 改指向了新 `utterance_id` |
| Python 读回 | `tokens_json`、`orig_json` 内容都对 |
| 真库 | 大小、修改时间、校正行数、曲目数都没变 |

`reimport_song` 会拒绝往真库里写。

## 和 Python 刻意不同的一处：撤销

Python 的「重置为 GiNZA 原始」只是把原始分词填回表格，保存后**仍是一条校正**。
下次导入时，会把旧分词器的结果强行套回去，哪怕新分词器已经分对了。

这里的撤销写回原始分词并**删掉校正行**：撤销就等于没校正过。
确认放在按钮上点第二下，不弹窗。

## 界面

- KWIC 结果里校正过的行标 ✏
- 保存或撤销之后自动重新检索：合并出来的新词立刻能搜到
- 拆分按 Unicode 码点数算——`𠮷野家` 在 JS 里是 4 个 UTF-16 码元、3 个字，按 `.length` 切会切出半个字
- 词拼起来和原句对不上时给出提示但不拦着保存（Python 版也允许直接改表层形）：
  KWIC 的左右语境会退回按词拼接

## 验证到哪一步

| 层 | 怎么验的 | 结果 |
|---|---|---|
| 规则 | `corrections.rs` 10 个单元测试 | 通过 |
| IPC | `tests/commands.rs` 在真库的临时副本上：打开 → 合并 → 保存 → 按新词检索带 ✏ → 撤销 | 通过 |
| 跨工具 | Python 存校正 → Python 删歌 → Rust 重导（见上） | 通过 |
| 界面 | `app/dev-mock.html` 点过：右键打开、选词合并、保存的请求参数、保存后自动重新检索、撤销要点两下 | 通过 |
| 实机 | 用临时副本启动 release 版，想实际右键点一遍 | **没点成**：系统浮层一直占着前台，点击被拦 |

实机没点成意味着有一件事**没有**被验证：WebView2 里右键时 `preventDefault` 能不能压住浏览器自带的右键菜单。
逻辑和请求都验过了，剩下的只是这一处宿主行为。
