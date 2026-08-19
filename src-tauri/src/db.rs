use rusqlite::{Connection, Result};
use std::fs;
use tauri::{AppHandle, Manager};

const CURRENT_SCHEMA_VERSION: i32 = 10;

/// The full schema, created fresh on first launch. The database was reset
/// for v1 (July 2026): lyrics, artist info, and image bytes live as files
/// under the app data cache directory; the database only stores metadata.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS folders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    enabled INTEGER NOT NULL DEFAULT 1,
    scanned_at INTEGER
);

CREATE TABLE IF NOT EXISTS artists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    sort_name TEXT,
    bio TEXT,
    info_provider TEXT,
    image_provider TEXT,
    info_term TEXT,
    image_term TEXT,
    track_count INTEGER NOT NULL DEFAULT 0,
    album_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    year INTEGER,
    mbid TEXT,
    cover_art_url TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL UNIQUE,
    title TEXT,
    track_number INTEGER,
    disc_number INTEGER,
    duration_ms INTEGER,
    year INTEGER,
    genre TEXT,
    album_id INTEGER REFERENCES albums(id) ON DELETE SET NULL,
    embedded_lyrics TEXT,
    lrc_offset_ms INTEGER NOT NULL DEFAULT 0,
    file_mtime INTEGER NOT NULL DEFAULT 0,
    lyrics_source TEXT,
    audio_format TEXT,
    audio_bitrate_kbps INTEGER,
    sample_rate_hz INTEGER,
    bit_depth INTEGER,
    channels INTEGER,
    file_size_bytes INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Sound Check analysis is derived data. It is keyed to the exact file
-- revision and analyzer version so stale measurements are never applied.
CREATE TABLE IF NOT EXISTS track_loudness (
    track_id INTEGER NOT NULL PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('complete', 'peak_only', 'silent', 'failed')),
    integrated_lufs REAL,
    true_peak_dbtp REAL,
    gain_db REAL,
    analyzed_file_mtime INTEGER NOT NULL,
    analyzed_file_size_bytes INTEGER,
    analyzer_version INTEGER NOT NULL,
    analyzed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    retry_after INTEGER,
    error_code TEXT,
    CHECK (
        (status = 'complete' AND integrated_lufs IS NOT NULL
            AND true_peak_dbtp IS NOT NULL AND gain_db IS NOT NULL
            AND error_code IS NULL)
        OR (status = 'peak_only' AND integrated_lufs IS NULL
            AND true_peak_dbtp IS NOT NULL AND gain_db IS NOT NULL
            AND error_code IS NULL)
        OR (status = 'silent' AND integrated_lufs IS NULL
            AND true_peak_dbtp IS NULL AND gain_db = 0
            AND error_code IS NULL)
        OR (status = 'failed' AND integrated_lufs IS NULL
            AND true_peak_dbtp IS NULL AND gain_db IS NULL
            AND error_code IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_track_loudness_status_retry
    ON track_loudness(status, retry_after);

CREATE TABLE IF NOT EXISTS album_artists (
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    PRIMARY KEY (album_id, artist_id)
);

CREATE TABLE IF NOT EXISTS track_artists (
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'main',
    PRIMARY KEY (track_id, artist_id, role)
);

CREATE TABLE IF NOT EXISTS artist_albums (
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    PRIMARY KEY (artist_id, album_id, role)
);

CREATE TABLE IF NOT EXISTS playlists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    smart_query TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, track_id)
);

CREATE TABLE IF NOT EXISTS play_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Online-metadata caches. Lyrics and artist info are small text kept in the
-- database (lyrics must be searchable); image bytes live as files under the
-- cache directory (see cache.rs) with only metadata here.
CREATE TABLE IF NOT EXISTS lyrics (
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    synced_text TEXT,
    plain_text TEXT,
    fetched_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (track_id, source)
);

CREATE TABLE IF NOT EXISTS artist_info (
    artist_id INTEGER NOT NULL PRIMARY KEY REFERENCES artists(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    file_path TEXT,
    fetched_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS images (
    entity_type TEXT NOT NULL,
    entity_id INTEGER NOT NULL,
    source TEXT NOT NULL,
    url TEXT,
    file_path TEXT,
    mime_type TEXT,
    fetched_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (entity_type, entity_id, source)
);

-- Playback observability has two deliberately separate layers. `listens` is
-- the query-friendly materialization of actual heard time; `playback_events`
-- is the immutable semantic transition trace that explains each listen.
CREATE TABLE IF NOT EXISTS listens (
    id TEXT NOT NULL PRIMARY KEY,
    session_id TEXT NOT NULL,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms > 0),
    ended_at_ms INTEGER,
    last_activity_at_ms INTEGER NOT NULL CHECK (last_activity_at_ms > 0),
    start_position_ms INTEGER NOT NULL DEFAULT 0 CHECK (start_position_ms >= 0),
    end_position_ms INTEGER NOT NULL DEFAULT 0 CHECK (end_position_ms >= 0),
    duration_ms INTEGER NOT NULL DEFAULT 0 CHECK (duration_ms >= 0),
    listened_ms INTEGER NOT NULL DEFAULT 0 CHECK (listened_ms >= 0),
    meaningful INTEGER NOT NULL DEFAULT 0 CHECK (meaningful IN (0, 1)),
    completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
    finalized INTEGER NOT NULL DEFAULT 0 CHECK (finalized IN (0, 1)),
    start_source TEXT NOT NULL,
    start_reason TEXT NOT NULL,
    end_reason TEXT,
    context_type TEXT NOT NULL DEFAULT 'unknown',
    context_id TEXT,
    queue_index INTEGER,
    play_order_index INTEGER,
    queue_length INTEGER NOT NULL DEFAULT 0 CHECK (queue_length >= 0),
    shuffle INTEGER NOT NULL DEFAULT 0 CHECK (shuffle IN (0, 1)),
    repeat_mode TEXT NOT NULL DEFAULT 'off'
);

CREATE INDEX IF NOT EXISTS idx_listens_track ON listens(track_id);
CREATE INDEX IF NOT EXISTS idx_listens_track_started
    ON listens(track_id, started_at_ms);
CREATE INDEX IF NOT EXISTS idx_listens_started ON listens(started_at_ms);
CREATE INDEX IF NOT EXISTS idx_listens_session ON listens(session_id, started_at_ms);
CREATE INDEX IF NOT EXISTS idx_listens_meaningful_started
    ON listens(meaningful, finalized, started_at_ms);
CREATE INDEX IF NOT EXISTS idx_listens_end_reason ON listens(end_reason, started_at_ms);
CREATE INDEX IF NOT EXISTS idx_listens_source_started
    ON listens(start_source, started_at_ms);
CREATE INDEX IF NOT EXISTS idx_listens_context_started
    ON listens(context_type, context_id, started_at_ms);

CREATE TABLE IF NOT EXISTS playback_events (
    id TEXT NOT NULL PRIMARY KEY,
    listen_id TEXT REFERENCES listens(id) ON DELETE CASCADE,
    session_id TEXT,
    occurred_at_ms INTEGER NOT NULL CHECK (occurred_at_ms > 0),
    event_type TEXT NOT NULL,
    source TEXT NOT NULL,
    reason TEXT,
    track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
    position_ms INTEGER,
    target_position_ms INTEGER,
    context_type TEXT NOT NULL DEFAULT 'unknown',
    context_id TEXT,
    queue_index INTEGER,
    play_order_index INTEGER,
    queue_length INTEGER NOT NULL DEFAULT 0 CHECK (queue_length >= 0),
    shuffle INTEGER NOT NULL DEFAULT 0 CHECK (shuffle IN (0, 1)),
    repeat_mode TEXT NOT NULL DEFAULT 'off'
);

CREATE INDEX IF NOT EXISTS idx_playback_events_occurred
    ON playback_events(occurred_at_ms);
CREATE INDEX IF NOT EXISTS idx_playback_events_listen
    ON playback_events(listen_id, occurred_at_ms);
CREATE INDEX IF NOT EXISTS idx_playback_events_session
    ON playback_events(session_id, occurred_at_ms);
CREATE INDEX IF NOT EXISTS idx_playback_events_type
    ON playback_events(event_type, occurred_at_ms);
CREATE INDEX IF NOT EXISTS idx_playback_events_source
    ON playback_events(source, occurred_at_ms);

-- Uploaded Discord artwork is user-owned presence metadata. It deliberately
-- lives outside normal cache cleanup so clearing/refetching album art cannot
-- cause repeat uploads to the configured artwork store.
CREATE TABLE IF NOT EXISTS discord_artwork_cache (
    cache_key TEXT NOT NULL PRIMARY KEY,
    catbox_url TEXT,
    s3_url TEXT,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK (catbox_url IS NOT NULL OR s3_url IS NOT NULL)
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_track_artists_artist_role ON track_artists(artist_id, role);
CREATE INDEX IF NOT EXISTS idx_track_artists_track_role ON track_artists(track_id, role);
CREATE INDEX IF NOT EXISTS idx_album_artists_artist ON album_artists(artist_id);
CREATE INDEX IF NOT EXISTS idx_album_artists_album ON album_artists(album_id);
CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album_id);
CREATE INDEX IF NOT EXISTS idx_artists_name ON artists(name);
CREATE INDEX IF NOT EXISTS idx_albums_title ON albums(title);
CREATE INDEX IF NOT EXISTS idx_artist_albums_artist ON artist_albums(artist_id);
CREATE INDEX IF NOT EXISTS idx_artist_albums_album ON artist_albums(album_id);
"#;

const SCHEMA_VERSION_TABLE: &str =
    "CREATE TABLE IF NOT EXISTS _schema_version (version INTEGER PRIMARY KEY);";

/// v7 stored one URL per artwork key without recording its provider. Catbox's
/// returned filename cannot be derived from the artwork hash, so preserve
/// known Catbox URLs exactly and retain other public URLs as S3 entries.
const V7_TO_V8: &str = r#"
ALTER TABLE discord_artwork_cache RENAME TO discord_artwork_cache_v7;

CREATE TABLE discord_artwork_cache (
    cache_key TEXT NOT NULL PRIMARY KEY,
    catbox_url TEXT,
    s3_url TEXT,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK (catbox_url IS NOT NULL OR s3_url IS NOT NULL)
);

INSERT INTO discord_artwork_cache (cache_key, catbox_url, s3_url, updated_at)
SELECT
    cache_key,
    CASE WHEN url LIKE 'https://files.catbox.moe/%' THEN url END,
    CASE WHEN url NOT LIKE 'https://files.catbox.moe/%' THEN url END,
    updated_at
FROM discord_artwork_cache_v7;

DROP TABLE discord_artwork_cache_v7;
"#;

/// v9 replaces the lossy `play_history` table with checkpointed listen facts
/// and a correlated semantic event trace. Legacy rows already passed the old
/// meaningful-listen filter, so migration marks them meaningful and preserves
/// the old 20-minute session grouping without inventing unavailable details.
const V8_TO_V9: &str = r#"
CREATE TABLE listens (
    id TEXT NOT NULL PRIMARY KEY,
    session_id TEXT NOT NULL,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms > 0),
    ended_at_ms INTEGER,
    last_activity_at_ms INTEGER NOT NULL CHECK (last_activity_at_ms > 0),
    start_position_ms INTEGER NOT NULL DEFAULT 0 CHECK (start_position_ms >= 0),
    end_position_ms INTEGER NOT NULL DEFAULT 0 CHECK (end_position_ms >= 0),
    duration_ms INTEGER NOT NULL DEFAULT 0 CHECK (duration_ms >= 0),
    listened_ms INTEGER NOT NULL DEFAULT 0 CHECK (listened_ms >= 0),
    meaningful INTEGER NOT NULL DEFAULT 0 CHECK (meaningful IN (0, 1)),
    completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
    finalized INTEGER NOT NULL DEFAULT 0 CHECK (finalized IN (0, 1)),
    start_source TEXT NOT NULL,
    start_reason TEXT NOT NULL,
    end_reason TEXT,
    context_type TEXT NOT NULL DEFAULT 'unknown',
    context_id TEXT,
    queue_index INTEGER,
    play_order_index INTEGER,
    queue_length INTEGER NOT NULL DEFAULT 0 CHECK (queue_length >= 0),
    shuffle INTEGER NOT NULL DEFAULT 0 CHECK (shuffle IN (0, 1)),
    repeat_mode TEXT NOT NULL DEFAULT 'off'
);

CREATE TABLE playback_events (
    id TEXT NOT NULL PRIMARY KEY,
    listen_id TEXT REFERENCES listens(id) ON DELETE CASCADE,
    session_id TEXT,
    occurred_at_ms INTEGER NOT NULL CHECK (occurred_at_ms > 0),
    event_type TEXT NOT NULL,
    source TEXT NOT NULL,
    reason TEXT,
    track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
    position_ms INTEGER,
    target_position_ms INTEGER,
    context_type TEXT NOT NULL DEFAULT 'unknown',
    context_id TEXT,
    queue_index INTEGER,
    play_order_index INTEGER,
    queue_length INTEGER NOT NULL DEFAULT 0 CHECK (queue_length >= 0),
    shuffle INTEGER NOT NULL DEFAULT 0 CHECK (shuffle IN (0, 1)),
    repeat_mode TEXT NOT NULL DEFAULT 'off'
);

CREATE TEMP TABLE _analytics_migration_nonce (value TEXT NOT NULL);
INSERT INTO _analytics_migration_nonce VALUES (lower(hex(randomblob(16))));

CREATE TEMP TABLE _legacy_listens AS
WITH ordered AS (
    SELECT
        ph.*,
        CASE
            WHEN LAG(started_at + played_ms / 1000)
                    OVER (ORDER BY started_at, id) IS NULL
              OR started_at - LAG(started_at + played_ms / 1000)
                    OVER (ORDER BY started_at, id) > 1200
            THEN 1 ELSE 0
        END AS new_session
    FROM play_history ph
), grouped AS (
    SELECT
        ordered.*,
        SUM(new_session) OVER (ORDER BY started_at, id) AS session_number
    FROM ordered
)
SELECT * FROM grouped;

INSERT INTO listens (
    id, session_id, track_id, started_at_ms, ended_at_ms,
    last_activity_at_ms, start_position_ms, end_position_ms, duration_ms,
    listened_ms, meaningful, completed, finalized, start_source,
    start_reason, end_reason, context_type, context_id, queue_index,
    play_order_index, queue_length, shuffle, repeat_mode
)
SELECT
    nonce.value || '-listen-' || legacy.id,
    nonce.value || '-session-' || legacy.session_number,
    legacy.track_id,
    legacy.started_at * 1000,
    legacy.started_at * 1000 + legacy.played_ms,
    legacy.started_at * 1000 + legacy.played_ms,
    0,
    0,
    COALESCE(t.duration_ms, 0),
    legacy.played_ms,
    1,
    legacy.completed,
    1,
    'legacy',
    'legacy_migration',
    'legacy_migration',
    'unknown',
    NULL,
    NULL,
    NULL,
    0,
    0,
    'off'
FROM _legacy_listens legacy
JOIN tracks t ON t.id = legacy.track_id
CROSS JOIN _analytics_migration_nonce nonce;

DROP TABLE _legacy_listens;
DROP TABLE _analytics_migration_nonce;
DROP TABLE play_history;

CREATE INDEX idx_listens_track ON listens(track_id);
CREATE INDEX idx_listens_track_started ON listens(track_id, started_at_ms);
CREATE INDEX idx_listens_started ON listens(started_at_ms);
CREATE INDEX idx_listens_session ON listens(session_id, started_at_ms);
CREATE INDEX idx_listens_meaningful_started
    ON listens(meaningful, finalized, started_at_ms);
CREATE INDEX idx_listens_end_reason ON listens(end_reason, started_at_ms);
CREATE INDEX idx_listens_source_started ON listens(start_source, started_at_ms);
CREATE INDEX idx_listens_context_started ON listens(context_type, context_id, started_at_ms);
CREATE INDEX idx_playback_events_occurred
    ON playback_events(occurred_at_ms);
CREATE INDEX idx_playback_events_listen
    ON playback_events(listen_id, occurred_at_ms);
CREATE INDEX idx_playback_events_session
    ON playback_events(session_id, occurred_at_ms);
CREATE INDEX idx_playback_events_type
    ON playback_events(event_type, occurred_at_ms);
CREATE INDEX idx_playback_events_source
    ON playback_events(source, occurred_at_ms);
"#;

/// v10 adds versioned, file-revision-bound EBU R128 measurements. These are
/// intentionally separate from user metadata because they can be rebuilt.
const V9_TO_V10: &str = r#"
CREATE TABLE track_loudness (
    track_id INTEGER NOT NULL PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('complete', 'peak_only', 'silent', 'failed')),
    integrated_lufs REAL,
    true_peak_dbtp REAL,
    gain_db REAL,
    analyzed_file_mtime INTEGER NOT NULL,
    analyzed_file_size_bytes INTEGER,
    analyzer_version INTEGER NOT NULL,
    analyzed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    retry_after INTEGER,
    error_code TEXT,
    CHECK (
        (status = 'complete' AND integrated_lufs IS NOT NULL
            AND true_peak_dbtp IS NOT NULL AND gain_db IS NOT NULL
            AND error_code IS NULL)
        OR (status = 'peak_only' AND integrated_lufs IS NULL
            AND true_peak_dbtp IS NOT NULL AND gain_db IS NOT NULL
            AND error_code IS NULL)
        OR (status = 'silent' AND integrated_lufs IS NULL
            AND true_peak_dbtp IS NULL AND gain_db = 0
            AND error_code IS NULL)
        OR (status = 'failed' AND integrated_lufs IS NULL
            AND true_peak_dbtp IS NULL AND gain_db IS NULL
            AND error_code IS NOT NULL)
    )
);

CREATE INDEX idx_track_loudness_status_retry
    ON track_loudness(status, retry_after);
"#;

/// Returns the profile-specific application data directory.
///
/// Release builds keep the existing Tauri directory so installed users do
/// not lose their library. Debug builds use a sibling directory instead,
/// preventing `tauri dev` from opening or modifying production data.
pub fn data_dir(app: &AppHandle) -> std::path::PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .expect("failed to get app data dir");
    if !cfg!(debug_assertions) {
        return base;
    }

    let Some(name) = base.file_name() else {
        return base.join("dev");
    };
    let mut dev_name = name.to_os_string();
    dev_name.push("-dev");
    base.with_file_name(dev_name)
}

pub fn db_path(app: &AppHandle) -> std::path::PathBuf {
    let dir = data_dir(app);
    fs::create_dir_all(&dir).expect("failed to create app data dir");
    dir.join("sparkle.db")
}

/// Opens a connection with WAL journaling and a busy timeout. WAL allows
/// readers to proceed while another connection is writing, so a long-running
/// scan on its own connection no longer blocks the UI.
pub fn open_connection(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(conn)
}

/// Opens (creating if needed) the library database. The bool is true when
/// the database was created fresh. The schema version read must never be
/// allowed to fail silently — treating a transient read error as "version 0"
/// would misreport an existing database as fresh.
pub fn init_db(app: &AppHandle) -> Result<(Connection, bool)> {
    let path = db_path(app);
    let conn = open_connection(&path)?;
    conn.execute(SCHEMA_VERSION_TABLE, [])?;
    let version: i32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
        [],
        |row| row.get(0),
    )?;
    if version == 0 {
        // Fresh database: create the current schema in one shot.
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(SCHEMA)?;
        record_schema_version(&tx, CURRENT_SCHEMA_VERSION)?;
        tx.commit()?;
        Ok((conn, true))
    } else {
        if version > CURRENT_SCHEMA_VERSION || version < 7 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut migrated = version;
        while migrated < CURRENT_SCHEMA_VERSION {
            match migrated {
                7 => migrate_v7_to_v8(&conn)?,
                8 => migrate_v8_to_v9(&conn)?,
                9 => migrate_v9_to_v10(&conn)?,
                _ => return Err(rusqlite::Error::InvalidQuery),
            }
            migrated += 1;
        }
        Ok((conn, false))
    }
}

fn migrate_v7_to_v8(conn: &Connection) -> Result<()> {
    log::info!(target: "sparkle::database", "event=migration_started from=7 to=8");
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(V7_TO_V8)?;
    record_schema_version(&tx, 8)?;
    tx.commit()?;
    log::info!(target: "sparkle::database", "event=migration_completed from=7 to=8");
    Ok(())
}

fn migrate_v8_to_v9(conn: &Connection) -> Result<()> {
    log::info!(target: "sparkle::database", "event=migration_started from=8 to=9");
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(V8_TO_V9)?;
    record_schema_version(&tx, 9)?;
    tx.commit()?;
    log::info!(target: "sparkle::database", "event=migration_completed from=8 to=9");
    Ok(())
}

fn migrate_v9_to_v10(conn: &Connection) -> Result<()> {
    log::info!(target: "sparkle::database", "event=migration_started from=9 to=10");
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(V9_TO_V10)?;
    record_schema_version(&tx, 10)?;
    tx.commit()?;
    log::info!(target: "sparkle::database", "event=migration_completed from=9 to=10");
    Ok(())
}

/// Finalize checkpoints left open by a crash or forced process termination.
/// A normal shutdown closes its active listen explicitly before the writer
/// durability barrier, so any remaining row is unambiguously interrupted.
pub fn recover_interrupted_listens(conn: &Connection) -> Result<usize> {
    conn.execute(
        "UPDATE listens SET \
         finalized = 1, \
         ended_at_ms = last_activity_at_ms, \
         meaningful = CASE \
             WHEN listened_ms >= 30000 OR \
                  (duration_ms > 0 AND listened_ms >= 5000 AND listened_ms * 2 >= duration_ms) \
             THEN 1 ELSE 0 END, \
         completed = CASE \
             WHEN duration_ms > 0 AND end_position_ms * 10 >= duration_ms * 9 \
             THEN 1 ELSE 0 END, \
         end_reason = 'interrupted' \
         WHERE finalized = 0",
        [],
    )
}

/// Upgraded databases retain their applied-version history. Recording only the
/// newly applied version avoids rewriting distinct primary keys to one value.
fn record_schema_version(conn: &Connection, version: i32) -> Result<()> {
    conn.execute(
        "INSERT INTO _schema_version (version) VALUES (?1)",
        [version],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn apply_schema(conn: &Connection) {
        conn.execute_batch(SCHEMA).unwrap();
    }

    #[test]
    fn schema_creates_all_tables() {
        let conn = Connection::open_in_memory().unwrap();
        apply_schema(&conn);
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for expected in [
            "album_artists",
            "albums",
            "artist_albums",
            "artist_info",
            "artists",
            "discord_artwork_cache",
            "folders",
            "images",
            "lyrics",
            "listens",
            "playback_events",
            "play_queue",
            "playlist_tracks",
            "playlists",
            "settings",
            "track_artists",
            "track_loudness",
            "tracks",
        ] {
            assert!(
                tables.iter().any(|t| t == expected),
                "missing table {expected}"
            );
        }
    }

    #[test]
    fn tracks_have_lyrics_source_override_column() {
        let conn = Connection::open_in_memory().unwrap();
        apply_schema(&conn);
        let mut stmt = conn.prepare("PRAGMA table_info(tracks)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(cols.iter().any(|c| c == "lyrics_source"));
    }

    #[test]
    fn playback_analytics_schema_has_query_and_trace_layers() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        apply_schema(&conn);
        let listen_columns = table_columns(&conn, "listens");
        let event_columns = table_columns(&conn, "playback_events");
        for expected in [
            "session_id",
            "listened_ms",
            "meaningful",
            "start_source",
            "start_reason",
            "end_reason",
            "context_type",
            "queue_index",
            "play_order_index",
        ] {
            assert!(listen_columns.iter().any(|column| column == expected));
        }
        for expected in [
            "listen_id",
            "session_id",
            "event_type",
            "source",
            "target_position_ms",
            "queue_index",
            "play_order_index",
        ] {
            assert!(event_columns.iter().any(|column| column == expected));
        }
    }

    fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut statement = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap()
    }

    #[test]
    fn v7_to_v8_preserves_legacy_provider_urls() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_VERSION_TABLE).unwrap();
        conn.execute_batch(
            "
            INSERT INTO _schema_version (version)
            VALUES (3), (4), (5), (6), (7);
            ",
        )
        .unwrap();
        conn.execute_batch(
            "
            CREATE TABLE discord_artwork_cache (
                cache_key TEXT NOT NULL PRIMARY KEY,
                url TEXT NOT NULL,
                updated_at INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO discord_artwork_cache (cache_key, url, updated_at)
            VALUES
                ('album:42', 'https://files.catbox.moe/legacy.jpg', 123),
                ('hash', 'https://cdn.example.test/artwork/hash.jpg', 456);
            ",
        )
        .unwrap();

        migrate_v7_to_v8(&conn).unwrap();

        let mut statement = conn
            .prepare(
                "SELECT cache_key, catbox_url, s3_url, updated_at
                 FROM discord_artwork_cache ORDER BY cache_key",
            )
            .unwrap();
        let mut rows = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap();
        let first: (String, Option<String>, Option<String>, i64) = rows.next().unwrap().unwrap();
        let second: (String, Option<String>, Option<String>, i64) = rows.next().unwrap().unwrap();
        assert_eq!(
            first,
            (
                "album:42".to_string(),
                Some("https://files.catbox.moe/legacy.jpg".to_string()),
                None,
                123
            )
        );
        assert_eq!(
            second,
            (
                "hash".to_string(),
                None,
                Some("https://cdn.example.test/artwork/hash.jpg".to_string()),
                456
            )
        );
        let mut statement = conn
            .prepare("SELECT version FROM _schema_version ORDER BY version")
            .unwrap();
        let versions = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<i32>>>()
            .unwrap();
        assert_eq!(versions, vec![3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn v8_to_v9_preserves_history_and_recovers_session_groups() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        conn.execute_batch(SCHEMA_VERSION_TABLE).unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks (
                id INTEGER PRIMARY KEY,
                duration_ms INTEGER
             );
             CREATE TABLE play_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                started_at INTEGER NOT NULL,
                played_ms INTEGER NOT NULL,
                completed INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO tracks (id, duration_ms) VALUES (1, 180000), (2, 240000);
             INSERT INTO play_history (track_id, started_at, played_ms, completed) VALUES
                (1, 1000, 60000, 0),
                (2, 1100, 200000, 1),
                (1, 4000, 45000, 0);",
        )
        .unwrap();

        migrate_v8_to_v9(&conn).unwrap();

        let rows: Vec<(i64, i64, bool, String)> = conn
            .prepare(
                "SELECT started_at_ms, listened_ms, completed, session_id \
                 FROM listens ORDER BY started_at_ms",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get::<_, i64>(2)? != 0,
                    row.get(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, 1_000_000);
        assert_eq!(rows[0].1, 60_000);
        assert!(rows[1].2);
        assert_eq!(rows[0].3, rows[1].3);
        assert_ne!(rows[1].3, rows[2].3);
        assert_eq!(recover_interrupted_listens(&conn).unwrap(), 0);
        conn.execute(
            "UPDATE listens SET finalized = 0, ended_at_ms = NULL, \
             listened_ms = 4000, end_position_ms = 10000, meaningful = 1, \
             completed = 1, end_reason = NULL WHERE started_at_ms = 1000000",
            [],
        )
        .unwrap();
        assert_eq!(recover_interrupted_listens(&conn).unwrap(), 1);
        let recovered: (i64, i64, i64, i64, String) = conn
            .query_row(
                "SELECT finalized, ended_at_ms, meaningful, completed, end_reason \
                 FROM listens WHERE started_at_ms = 1000000",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(recovered, (1, 1_060_000, 0, 0, "interrupted".into()));
        let legacy_table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'play_history'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_table, 0);
    }

    #[test]
    fn v9_to_v10_adds_versioned_loudness_analysis() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        conn.execute_batch(SCHEMA_VERSION_TABLE).unwrap();
        conn.execute_batch(
            "INSERT INTO _schema_version (version) VALUES (9);
             CREATE TABLE tracks (id INTEGER PRIMARY KEY);",
        )
        .unwrap();

        migrate_v9_to_v10(&conn).unwrap();

        let columns = table_columns(&conn, "track_loudness");
        for expected in [
            "track_id",
            "integrated_lufs",
            "true_peak_dbtp",
            "gain_db",
            "analyzed_file_mtime",
            "analyzer_version",
            "attempt_count",
        ] {
            assert!(columns.iter().any(|column| column == expected));
        }
        let version: i32 = conn
            .query_row("SELECT MAX(version) FROM _schema_version", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, 10);
    }
}
