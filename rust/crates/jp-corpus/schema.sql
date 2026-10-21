-- 建一个空的语料库。
-- **这份是从迁移完成的真实库里导出来的**（schema 的事实来源是库本身，不是手写的副本）。
-- 重新生成：scratchpad/gen_schema.py。FTS5 的影子表和 sqlite_sequence 不在这里——它们由 SQLite 自己建。

CREATE TABLE IF NOT EXISTS albums (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    title                   TEXT NOT NULL,
    normalized_title        TEXT NOT NULL,
    album_artist            TEXT NOT NULL DEFAULT '',
    normalized_album_artist TEXT NOT NULL DEFAULT '',
    year                    TEXT NOT NULL DEFAULT '',
    artwork_path            TEXT NOT NULL DEFAULT '',
    created_at              TEXT NOT NULL DEFAULT '',
    release_group_mbid TEXT,
    label TEXT,
    barcode TEXT,
    album_type TEXT,
    track_count INTEGER,
    -- 同名专辑在不同歌手名下是两张不同的专辑
    UNIQUE (normalized_title, normalized_album_artist)
);

CREATE TABLE IF NOT EXISTS artists (
            name        TEXT PRIMARY KEY,
            image_path  TEXT,
            artist_type TEXT,
            country     TEXT,
            formed      TEXT,
            updated_at  TEXT
        , mbid TEXT, ended TEXT, has_ended INTEGER, image_manual INTEGER);

-- 乐队成员。band_id、member_id 都是 people.id。
-- 一条关系可能有三个来源：musicbrainz（刮削拿到的）、confirmed（推测之后你确认的）、
-- manual（你手动加的）。后两种自动流程永远不覆盖。
-- status = rejected 是「你否掉了」：留着这一行，下次刮削或推测就不会再把它加回来。
CREATE TABLE IF NOT EXISTS band_members (
    band_id    INTEGER NOT NULL,
    member_id  INTEGER NOT NULL,
    -- 在队里做什么：主唱、吉他、作词作曲……
    part       TEXT    NOT NULL DEFAULT '',
    -- 加入 / 离开（或乐队结束）的日期，YYYY、YYYY-MM 或 YYYY-MM-DD；离开为空 = 现在还在
    joined     TEXT    NOT NULL DEFAULT '',
    left_at    TEXT    NOT NULL DEFAULT '',
    source     TEXT    NOT NULL,
    status     TEXT    NOT NULL DEFAULT 'active',
    updated_at TEXT    NOT NULL DEFAULT '',
    PRIMARY KEY (band_id, member_id)
);
CREATE INDEX IF NOT EXISTS idx_band_members_member ON band_members(member_id);

CREATE TABLE IF NOT EXISTS chapters (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id   TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
            chapter_idx INTEGER NOT NULL,
            title       TEXT DEFAULT '',
            utt_start   INTEGER,
            utt_end     INTEGER
        );

CREATE TABLE IF NOT EXISTS dict_registry (
                name        TEXT PRIMARY KEY,
                zip_path    TEXT NOT NULL DEFAULT '',
                dict_type   TEXT NOT NULL DEFAULT 'terms',
                entry_count INTEGER NOT NULL DEFAULT 0,
                enabled     INTEGER NOT NULL DEFAULT 1,
                sort_order  INTEGER NOT NULL DEFAULT 999,
                revision    TEXT NOT NULL DEFAULT '',
                imported_at TEXT NOT NULL DEFAULT ''
            );

CREATE TABLE IF NOT EXISTS dict_terms (
                    id        INTEGER PRIMARY KEY AUTOINCREMENT,
                    term      TEXT NOT NULL,
                    reading   TEXT NOT NULL DEFAULT '',
                    dict_name TEXT NOT NULL,
                    defs_json TEXT NOT NULL DEFAULT '[]'
                );

CREATE TABLE IF NOT EXISTS favorites (
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (entity_type, entity_id)
);

CREATE TABLE IF NOT EXISTS jlpt_cache (lemma TEXT PRIMARY KEY, level TEXT NOT NULL DEFAULT '');

CREATE TABLE IF NOT EXISTS people (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    -- scraper.normalize.matching_key 的结果：NFKC + 折大小写 + 去标点空白。
    -- 「山口　一郎」和「山口一郎」靠它归成同一个人。
    normalized_name TEXT NOT NULL UNIQUE,
    sort_name       TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS play_history (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    song_id      TEXT NOT NULL,
    played_at    TEXT NOT NULL,
    -- 实际听了多久，用来区分「真的听完了」和「点开就切走」
    listened_sec REAL NOT NULL DEFAULT 0,
    position_sec REAL NOT NULL DEFAULT 0,
    completed    INTEGER NOT NULL DEFAULT 0,
    -- 从哪里播的：library / search / research，用于分析研究工作流
    source       TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS scrape_attempts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path     TEXT NOT NULL,
    provider      TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL,
    confidence    REAL NOT NULL DEFAULT 0,
    error_type    TEXT NOT NULL DEFAULT '',
    error_message TEXT NOT NULL DEFAULT '',
    attempted_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scrape_state (
    file_path       TEXT PRIMARY KEY,
    song_id         TEXT,
    status          TEXT NOT NULL,
    confidence      REAL NOT NULL DEFAULT 0,
    provider        TEXT NOT NULL DEFAULT '',
    provider_id     TEXT NOT NULL DEFAULT '',
    error_type      TEXT NOT NULL DEFAULT '',
    error_message   TEXT NOT NULL DEFAULT '',
    retry_count     INTEGER NOT NULL DEFAULT 0,
    first_seen_at   TEXT,
    last_attempt_at TEXT,
    resolved_at     TEXT,
    breakdown_json  TEXT,
    candidates_json TEXT,
    queries_json    TEXT
);

CREATE TABLE IF NOT EXISTS songs (
            id         TEXT PRIMARY KEY,
            title      TEXT NOT NULL,
            artist     TEXT NOT NULL,
            year       TEXT,
            album      TEXT,
            genre      TEXT,
            audio_path TEXT
        , corpus_type TEXT NOT NULL DEFAULT 'song', source_file TEXT, cover_path TEXT, duration_sec REAL, album_id INTEGER, added_at TEXT, release_date TEXT, language TEXT, isrc TEXT, recording_mbid TEXT, audio_fingerprint TEXT);

CREATE TABLE IF NOT EXISTS external_ids (
    entity_type TEXT, entity_id TEXT, provider TEXT, external_id TEXT, source TEXT, confidence REAL,
    PRIMARY KEY(entity_type, entity_id, provider)
);
CREATE INDEX IF NOT EXISTS idx_songs_audio_fingerprint ON songs(audio_fingerprint);
CREATE INDEX IF NOT EXISTS idx_external_ids_external_id ON external_ids(external_id);

CREATE TABLE IF NOT EXISTS token_corrections (
            utterance_id INTEGER PRIMARY KEY,
            tokens_json  TEXT NOT NULL,
            orig_json    TEXT NOT NULL DEFAULT '[]',
            text         TEXT NOT NULL DEFAULT '',
            song_artist  TEXT NOT NULL DEFAULT '',
            song_title   TEXT NOT NULL DEFAULT '',
            updated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

CREATE TABLE IF NOT EXISTS tokens (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            utterance_id INTEGER NOT NULL REFERENCES utterances(id),
            token_idx    INTEGER NOT NULL,
            surface      TEXT NOT NULL,
            lemma        TEXT NOT NULL,
            pos          TEXT,
            dep          TEXT,
            head         TEXT
        );

CREATE TABLE IF NOT EXISTS track_credits (
    song_id   TEXT    NOT NULL,
    person_id INTEGER NOT NULL,
    role      TEXT    NOT NULL,
    position  INTEGER NOT NULL DEFAULT 0,
    -- 来源要留痕：lrc 是从歌词解析的，manual 是人工改的，
    -- 将来 provider 刮到的可以标 provider。重跑回填时不能覆盖人工修正。
    source    TEXT    NOT NULL DEFAULT '',
    PRIMARY KEY (song_id, person_id, role)
);

CREATE TABLE IF NOT EXISTS track_original_metadata (
    file_path         TEXT PRIMARY KEY,
    song_id           TEXT,
    title             TEXT NOT NULL DEFAULT '',
    artist            TEXT NOT NULL DEFAULT '',
    album             TEXT NOT NULL DEFAULT '',
    year              TEXT NOT NULL DEFAULT '',
    genre             TEXT NOT NULL DEFAULT '',
    track_number      INTEGER,
    disc_number       INTEGER,
    duration_sec      REAL,
    size              INTEGER,
    mtime             REAL,
    normalized_title  TEXT NOT NULL DEFAULT '',
    normalized_artist TEXT NOT NULL DEFAULT '',
    normalized_album  TEXT NOT NULL DEFAULT '',
    source_json       TEXT,
    captured_at       TEXT
);

CREATE TABLE IF NOT EXISTS "utterances" (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            song_id    TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
            line_idx   INTEGER NOT NULL,
            time_sec   REAL,
            text       TEXT NOT NULL,
            chapter_id INTEGER REFERENCES chapters(id)
        , version_id INTEGER, time_confidence REAL, token_status TEXT, tokenizer_run_id INTEGER);

CREATE VIRTUAL TABLE IF NOT EXISTS utterances_fts USING fts5(
            text,
            content=utterances,
            content_rowid=id,
            tokenize='trigram'
        );

-- ── 语料质量与来源追踪（20261018）──────────────────────────────
-- 设计说明见 docs/provenance.md。

-- 歌词版本：每挂一次歌词记一版，旧版不删。
-- raw_text 是挂上时的原文，一个字不改——重新分词、校正分词都不碰它，
-- 将来要换解析规则也能从这里重来。老库补登记的版本原文没留下，是 NULL。
-- 当前在用的那一版 is_current = 1；utterances.version_id 指向它。
CREATE TABLE IF NOT EXISTS lyric_versions (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    song_id          TEXT    NOT NULL,
    -- 这首歌的第几版，从 1 起
    version_no       INTEGER NOT NULL,
    -- lrc_file（导入时旁边的 .lrc）/ embedded（音频内嵌）/ sibling（后来出现在旁边的 .lrc）/
    -- online（在线搜到的）/ manual_file（你选的文件）/ manual_edit（逐行编辑）/ restored（从旧版恢复）/
    -- legacy（老库补登记）
    source_type      TEXT    NOT NULL,
    -- 在线来源是 provider 名（netease），本地来源是文件名
    source_name      TEXT    NOT NULL DEFAULT '',
    -- 路径，或在线条目「曲名 — 歌手」
    source_ref       TEXT    NOT NULL DEFAULT '',
    imported_at      TEXT    NOT NULL DEFAULT '',
    raw_text         TEXT,
    -- 解析后「时间 + 文本」序列的指纹：同一份歌词重挂一次，指纹相同
    content_hash     TEXT    NOT NULL DEFAULT '',
    line_count       INTEGER NOT NULL DEFAULT 0,
    timed_line_count INTEGER NOT NULL DEFAULT 0,
    -- 0~1，校验估计的完整度（漏行、截断、乱码会扣分）
    completeness     REAL,
    -- 校验结果：问题清单、重复行比例、语言判断
    validation_json  TEXT    NOT NULL DEFAULT '{}',
    validated_at     TEXT    NOT NULL DEFAULT '',
    is_current       INTEGER NOT NULL DEFAULT 0,
    note             TEXT    NOT NULL DEFAULT '',
    UNIQUE (song_id, version_no)
);
CREATE INDEX IF NOT EXISTS idx_lyric_versions_song ON lyric_versions(song_id, is_current);
CREATE INDEX IF NOT EXISTS idx_utterances_version ON utterances(version_id);

-- 分词器：引擎、词典、切分模式、配置的一种组合一行。utterances.tokenizer_run_id 指向它。
-- fingerprint 由其余字段算出：词典或配置一变就是新的一行，旧分词仍指着旧的那行。
CREATE TABLE IF NOT EXISTS tokenizer_runs (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    fingerprint      TEXT    NOT NULL UNIQUE,
    engine           TEXT    NOT NULL,
    engine_version   TEXT    NOT NULL DEFAULT '',
    dict_description TEXT    NOT NULL DEFAULT '',
    dict_created_at  TEXT    NOT NULL DEFAULT '',
    dict_size        INTEGER NOT NULL DEFAULT 0,
    split_mode       TEXT    NOT NULL DEFAULT '',
    config_hash      TEXT    NOT NULL DEFAULT '',
    pos_scheme       TEXT    NOT NULL DEFAULT '',
    first_used_at    TEXT    NOT NULL DEFAULT ''
);

-- 修订历史：歌词换版、分词校正 / 撤销、重新分词、改曲目信息……每件事一行，只追加。
-- 删歌时**不删**这里的行：它是日志，歌手 + 曲名另存一份，删了也看得出是哪首。
CREATE TABLE IF NOT EXISTS corpus_revisions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    song_id      TEXT    NOT NULL,
    song_artist  TEXT    NOT NULL DEFAULT '',
    song_title   TEXT    NOT NULL DEFAULT '',
    -- lyrics_attached / lyrics_replaced / lyrics_edited / tokens_corrected / tokens_reverted /
    -- corrections_restored / retokenized / metadata_edited / song_deleted / legacy_registered
    kind         TEXT    NOT NULL,
    version_id   INTEGER,
    utterance_id INTEGER,
    line_idx     INTEGER,
    before_json  TEXT    NOT NULL DEFAULT '',
    after_json   TEXT    NOT NULL DEFAULT '',
    -- user（你在界面上做的）/ import / scraper / system
    actor        TEXT    NOT NULL DEFAULT '',
    note         TEXT    NOT NULL DEFAULT '',
    created_at   TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_revisions_song ON corpus_revisions(song_id, id);

-- 元数据字段的来源和置信度：songs 的年份、流派、专辑、语言……各是从哪来的；
-- entity_type = artist 时 entity_id 是歌手名，记刮削到的类型、国家、成立 / 结束、MBID、照片。
-- source：tag（文件标签）/ filename（从文件名、目录猜的）/ manual（你改的）/
-- itunes、musicbrainz……（刮削）/ heuristic（程序按歌词推断的）。
-- confidence 为 NULL 表示「不适用」（标签原值、手改的值没有置信度一说）。
CREATE TABLE IF NOT EXISTS field_provenance (
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    field       TEXT NOT NULL,
    source      TEXT NOT NULL,
    source_ref  TEXT NOT NULL DEFAULT '',
    confidence  REAL,
    updated_at  TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (entity_type, entity_id, field)
);

-- 存起来的子语料：一组筛选条件（年代、歌手、流派、语言、完整度……）起个名字。
CREATE TABLE IF NOT EXISTS subcorpora (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    filter_json TEXT NOT NULL,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT '',
    updated_at  TEXT NOT NULL DEFAULT ''
);

-- 分析记录：一次统计的条件、范围、分词器、口径、结果，全都存下来，能原样重跑、能对比。
-- scope_json 是当时纳入的每首歌和它的歌词版本；tokenizers_json 是这些歌词行用过的分词器。
CREATE TABLE IF NOT EXISTS analysis_runs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    -- corpus_report / frequency
    kind            TEXT    NOT NULL,
    label           TEXT    NOT NULL DEFAULT '',
    filter_json     TEXT    NOT NULL,
    params_json     TEXT    NOT NULL DEFAULT '{}',
    scope_json      TEXT    NOT NULL,
    scope_hash      TEXT    NOT NULL DEFAULT '',
    tokenizers_json TEXT    NOT NULL DEFAULT '[]',
    -- 统计口径：词元怎么取、排除哪些词性、TTR / STTR 怎么算
    method_json     TEXT    NOT NULL DEFAULT '{}',
    result_json     TEXT    NOT NULL,
    result_hash     TEXT    NOT NULL DEFAULT '',
    schema_version  INTEGER NOT NULL DEFAULT 0,
    app_version     TEXT    NOT NULL DEFAULT '',
    -- 重跑出来的那次记着原来是哪一次
    rerun_of        INTEGER,
    created_at      TEXT    NOT NULL DEFAULT '',
    -- 当时的库指纹（analysis::snapshot）
    snapshot_json   TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_analysis_runs_time ON analysis_runs(created_at);

CREATE TABLE IF NOT EXISTS yomitan_freq (
                term    TEXT PRIMARY KEY,
                freq    INTEGER NOT NULL DEFAULT 0
            );

CREATE TABLE IF NOT EXISTS yomitan_meta (
                key TEXT PRIMARY KEY,
                val TEXT
            );

CREATE TABLE IF NOT EXISTS yomitan_pitch (
                term    TEXT NOT NULL,
                reading TEXT NOT NULL DEFAULT '',
                positions TEXT NOT NULL DEFAULT '[]',
                PRIMARY KEY (term, reading)
            );

CREATE TABLE IF NOT EXISTS yomitan_zh (
                term    TEXT PRIMARY KEY,
                reading TEXT NOT NULL DEFAULT '',
                zh_defs TEXT NOT NULL DEFAULT '[]'
            );

CREATE INDEX IF NOT EXISTS idx_albums_norm     ON albums(normalized_title);

CREATE INDEX IF NOT EXISTS idx_chapters_source ON chapters(source_id);

CREATE INDEX IF NOT EXISTS idx_credits_person  ON track_credits(person_id, role);

CREATE INDEX IF NOT EXISTS idx_credits_role    ON track_credits(role);

CREATE INDEX IF NOT EXISTS idx_credits_song    ON track_credits(song_id);

CREATE INDEX IF NOT EXISTS idx_dt_reading ON dict_terms(reading);

CREATE INDEX IF NOT EXISTS idx_dt_term_dict ON dict_terms(term, dict_name);

CREATE INDEX IF NOT EXISTS idx_history_song    ON play_history(song_id, played_at);

CREATE INDEX IF NOT EXISTS idx_history_time    ON play_history(played_at);

CREATE INDEX IF NOT EXISTS idx_people_norm     ON people(normalized_name);

CREATE INDEX IF NOT EXISTS idx_scrape_attempts_file ON scrape_attempts(file_path, attempted_at);

CREATE INDEX IF NOT EXISTS idx_scrape_state_song    ON scrape_state(song_id);

CREATE INDEX IF NOT EXISTS idx_scrape_state_status  ON scrape_state(status);

CREATE INDEX IF NOT EXISTS idx_songs_album ON songs(album_id);

CREATE INDEX IF NOT EXISTS idx_songs_corpus_type ON songs(corpus_type);

CREATE INDEX IF NOT EXISTS idx_tc_song_text ON token_corrections(song_artist, song_title, text);

CREATE INDEX IF NOT EXISTS idx_token_corr_lookup ON token_corrections(song_artist, song_title, text);

CREATE INDEX IF NOT EXISTS idx_tokens_lemma   ON tokens(lemma);

CREATE INDEX IF NOT EXISTS idx_tokens_lemma_utterance ON tokens(lemma, utterance_id);

CREATE INDEX IF NOT EXISTS idx_tokens_pos     ON tokens(pos);

CREATE INDEX IF NOT EXISTS idx_tokens_surface ON tokens(surface);

CREATE INDEX IF NOT EXISTS idx_tokens_utterance ON tokens(utterance_id);

CREATE INDEX IF NOT EXISTS idx_tom_song             ON track_original_metadata(song_id);

CREATE INDEX IF NOT EXISTS idx_utt_song_id ON utterances(song_id);

CREATE INDEX IF NOT EXISTS idx_utterances_song_time ON utterances(song_id, time_sec);

CREATE INDEX IF NOT EXISTS idx_yomitan_zh_reading ON yomitan_zh(reading);

-- 词级时间：一行歌词里某个词从第几秒开始唱（见 jp-corpus/src/word_timing.rs）。
-- 位置是这一行 text 里的字符下标 [char_start, char_end)。
-- source：lrc（导入的逐字时间）/ manual（手动校准，同一处两种都有时以它为准）
CREATE TABLE IF NOT EXISTS word_timings (
    utterance_id INTEGER NOT NULL REFERENCES utterances(id),
    char_start   INTEGER NOT NULL,
    char_end     INTEGER NOT NULL,
    start_sec    REAL    NOT NULL,
    end_sec      REAL,
    source       TEXT    NOT NULL,
    PRIMARY KEY (utterance_id, char_start, source)
);
