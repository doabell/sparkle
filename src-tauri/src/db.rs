use rusqlite::{Connection, Result};
use std::fs;
use tauri::{AppHandle, Manager};

const CURRENT_SCHEMA_VERSION: i32 = 8;

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

-- Play events for listening statistics ("sparkle unwrapped").
CREATE TABLE IF NOT EXISTS play_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    started_at INTEGER NOT NULL,
    played_ms INTEGER NOT NULL CHECK (played_ms >= 0),
    completed INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_play_history_track ON play_history(track_id);
CREATE INDEX IF NOT EXISTS idx_play_history_started ON play_history(started_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_play_history_event
    ON play_history(track_id, started_at, played_ms);

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

pub fn db_path(app: &AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .expect("failed to get app data dir");
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
    } else if version == CURRENT_SCHEMA_VERSION {
        Ok((conn, false))
    } else if version == CURRENT_SCHEMA_VERSION - 1 {
        migrate_v7_to_v8(&conn)?;
        Ok((conn, false))
    } else {
        Err(rusqlite::Error::InvalidQuery)
    }
}

fn migrate_v7_to_v8(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(V7_TO_V8)?;
    record_schema_version(&tx, CURRENT_SCHEMA_VERSION)?;
    tx.commit()
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
            "play_history",
            "play_queue",
            "playlist_tracks",
            "playlists",
            "settings",
            "track_artists",
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
}
