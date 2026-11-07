-- 在库视图：面向用户的读查询（曲库、检索、分析、首页、制卡例句、补全报告）都从这里读，
-- 被移出曲库（songs.deleted_at 不为空）的歌和挂在它上面的东西在这里消失，数据本身不动。
-- 导入、身份识别、来源追踪、迁移这些维护代码要看得见移出的歌（文件回来时才能恢复），继续读原表。
--
-- 写法统一是「排除被移出的」，而不是「连得上在库的歌」：没有歌被移出时，
-- 每个视图和原表逐行相同（包括连不上歌的孤儿行），所有数字和改之前一致。
--
-- 排除用的是不相关子查询 `NOT IN (SELECT … FROM removed_songs)`：SQLite 只算一次、建成临时索引，
-- 没有歌被移出时是空集，几乎不花时间。写成逐行 `NOT EXISTS` 的话，6.6 万个词每个都要回表查两次，
-- 实测整组分析接口慢 1.5 倍。外层列用 COALESCE 包一下：空值 NOT IN 非空集合会得 NULL，那一行就丢了。
--
-- 用 * 是有意的：SQLite 每次载入结构时重新展开，原表以后加的列视图里自动都有。
-- SQLite 建视图时不检查表和列在不在，用到时才查，所以这个文件可以单独跑。

-- 被移出的歌。走 schema.sql 里的部分索引 idx_songs_removed
CREATE VIEW IF NOT EXISTS removed_songs AS
    SELECT id FROM songs WHERE deleted_at IS NOT NULL AND id IS NOT NULL;

CREATE VIEW IF NOT EXISTS live_songs AS
    SELECT * FROM songs WHERE deleted_at IS NULL;

CREATE VIEW IF NOT EXISTS live_utterances AS
    SELECT * FROM utterances
    WHERE COALESCE(song_id, '') NOT IN (SELECT id FROM removed_songs);

-- 只给不经过 live_utterances 的统计用；已经连了 live_utterances 的查询不用再换它
CREATE VIEW IF NOT EXISTS live_tokens AS
    SELECT * FROM tokens
    WHERE COALESCE(utterance_id, -1) NOT IN (
        SELECT u.id FROM utterances u WHERE u.song_id IN (SELECT id FROM removed_songs));

CREATE VIEW IF NOT EXISTS live_credits AS
    SELECT * FROM track_credits
    WHERE COALESCE(song_id, '') NOT IN (SELECT id FROM removed_songs);

CREATE VIEW IF NOT EXISTS live_play_history AS
    SELECT * FROM play_history
    WHERE COALESCE(song_id, '') NOT IN (SELECT id FROM removed_songs);

-- 真删一首歌时，因此没有歌了的专辑、没有署名了的人会一起删掉（见 jp-import 的 delete_song）。
-- 这里对应地藏起「有歌（署名）、但全都被移出了」的那些；本来就没有的照常显示
CREATE VIEW IF NOT EXISTS live_albums AS
    SELECT * FROM albums a
    WHERE NOT EXISTS (SELECT 1 FROM songs s WHERE s.album_id = a.id)
       OR EXISTS (SELECT 1 FROM songs s WHERE s.album_id = a.id AND s.deleted_at IS NULL);

CREATE VIEW IF NOT EXISTS live_people AS
    SELECT * FROM people p
    WHERE NOT EXISTS (SELECT 1 FROM track_credits c WHERE c.person_id = p.id)
       OR EXISTS (SELECT 1 FROM live_credits c WHERE c.person_id = p.id);

-- 歌词版本（语料质量总览用）。真删会连版本记录一起删（lineage::forget_song）
CREATE VIEW IF NOT EXISTS live_lyric_versions AS
    SELECT * FROM lyric_versions
    WHERE COALESCE(song_id, '') NOT IN (SELECT id FROM removed_songs);

-- 刮削状态（刮削统计用）。按歌或按文件路径记，真删时两种都删
CREATE VIEW IF NOT EXISTS live_scrape_state AS
    SELECT * FROM scrape_state
    WHERE COALESCE(song_id, '') NOT IN (SELECT id FROM removed_songs)
      AND COALESCE(file_path, '') NOT IN (
          SELECT audio_path FROM songs WHERE deleted_at IS NOT NULL AND audio_path IS NOT NULL);
