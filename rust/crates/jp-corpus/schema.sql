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
        );

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
        , corpus_type TEXT NOT NULL DEFAULT 'song', source_file TEXT, cover_path TEXT, duration_sec REAL, album_id INTEGER);

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
        );

CREATE VIRTUAL TABLE IF NOT EXISTS utterances_fts USING fts5(
            text,
            content=utterances,
            content_rowid=id,
            tokenize='trigram'
        );

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
