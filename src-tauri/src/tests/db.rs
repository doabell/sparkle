use super::*;
use rusqlite::Connection;

pub(crate) fn test_connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn
}

#[test]
fn initialization_is_idempotent_and_rejects_unsupported_versions() {
    let (conn, fresh) = initialize_connection(Connection::open_in_memory().unwrap()).unwrap();
    assert!(fresh);
    conn.execute(
        "INSERT INTO tracks (file_path,title) VALUES ('song.flac','Keep me')",
        [],
    )
    .unwrap();
    let (conn, fresh) = initialize_connection(conn).unwrap();
    assert!(!fresh);
    assert_eq!(
        conn.query_row("SELECT title FROM tracks", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "Keep me"
    );
    for version in [1, 6, CURRENT_SCHEMA_VERSION + 1] {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(SCHEMA_VERSION_TABLE, []).unwrap();
        record_schema_version(&conn, version).unwrap();
        assert!(initialize_connection(conn).is_err());
    }
}

#[test]
fn opening_connections_enforces_foreign_keys_and_busy_timeout() {
    let conn = open_connection(std::path::Path::new(":memory:")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("PRAGMA busy_timeout", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        5000
    );
}

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
