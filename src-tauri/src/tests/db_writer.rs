use super::*;
use crate::analytics::{
    ListenEndReason, ListenStartReason, PlaybackContext, PlaybackEventKind, PlaybackSource,
};

fn create_test_schema(conn: &Connection) {
    conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
    conn.execute_batch(
        "CREATE TABLE settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
         );
         CREATE TABLE tracks (id INTEGER PRIMARY KEY);
         INSERT INTO tracks (id) VALUES (7), (42);
         CREATE TABLE listens (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER,
            last_activity_at_ms INTEGER NOT NULL,
            start_position_ms INTEGER NOT NULL,
            end_position_ms INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            listened_ms INTEGER NOT NULL,
            meaningful INTEGER NOT NULL,
            completed INTEGER NOT NULL,
            finalized INTEGER NOT NULL,
            start_source TEXT NOT NULL,
            start_reason TEXT NOT NULL,
            end_reason TEXT,
            context_type TEXT NOT NULL,
            context_id TEXT,
            queue_index INTEGER,
            play_order_index INTEGER,
            queue_length INTEGER NOT NULL,
            shuffle INTEGER NOT NULL,
            repeat_mode TEXT NOT NULL
         );
         CREATE TABLE playback_events (
            id TEXT PRIMARY KEY,
            listen_id TEXT REFERENCES listens(id) ON DELETE CASCADE,
            session_id TEXT,
            occurred_at_ms INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            source TEXT NOT NULL,
            reason TEXT,
            track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
            position_ms INTEGER,
            target_position_ms INTEGER,
            context_type TEXT NOT NULL,
            context_id TEXT,
            queue_index INTEGER,
            play_order_index INTEGER,
            queue_length INTEGER NOT NULL,
            shuffle INTEGER NOT NULL,
            repeat_mode TEXT NOT NULL
         );",
    )
    .unwrap();
}

fn sample_listen(id: &str, started_at_ms: i64, finalized: bool) -> ListenRecord {
    ListenRecord {
        id: id.to_string(),
        session_id: "session-1".to_string(),
        track_id: 7,
        started_at_ms,
        ended_at_ms: finalized.then_some(started_at_ms + 123_000),
        last_activity_at_ms: started_at_ms + 123_000,
        start_position_ms: 0,
        end_position_ms: 123_000,
        duration_ms: 180_000,
        listened_ms: 123_000,
        meaningful: true,
        completed: false,
        finalized,
        start_source: PlaybackSource::Ui,
        start_reason: ListenStartReason::QueueStarted,
        end_reason: finalized.then_some(ListenEndReason::ManualNext),
        context: PlaybackContext {
            kind: "album".to_string(),
            id: Some("9".to_string()),
        },
        queue_index: Some(0),
        play_order_index: Some(0),
        queue_length: 10,
        shuffle: false,
        repeat_mode: RepeatMode::Off,
    }
}

fn sample_event(id: &str, listen_id: &str, occurred_at_ms: i64) -> PlaybackEventRecord {
    PlaybackEventRecord {
        id: id.to_string(),
        listen_id: Some(listen_id.to_string()),
        session_id: Some("session-1".to_string()),
        occurred_at_ms,
        kind: PlaybackEventKind::ListenEnded,
        source: PlaybackSource::Ui,
        reason: Some("manual_next".to_string()),
        track_id: Some(7),
        position_ms: Some(123_000),
        target_position_ms: None,
        context: PlaybackContext::default(),
        queue_index: Some(0),
        play_order_index: Some(0),
        queue_length: 10,
        shuffle: false,
        repeat_mode: RepeatMode::Off,
    }
}

#[test]
fn listen_checkpoint_is_updated_by_final_record() {
    let conn = Connection::open_in_memory().unwrap();
    create_test_schema(&conn);
    let mut listen = sample_listen("listen-1", 1_700_000_000_000, false);
    listen.listened_ms = 30_000;
    listen.end_position_ms = 30_000;
    write_listen(&conn, &listen).unwrap();
    listen.listened_ms = 123_000;
    listen.end_position_ms = 123_000;
    listen.ended_at_ms = Some(1_700_000_123_000);
    listen.finalized = true;
    listen.end_reason = Some(ListenEndReason::ManualNext);
    write_listen(&conn, &listen).unwrap();
    let (listened_ms, finalized, reason): (i64, i64, String) = conn
        .query_row(
            "SELECT listened_ms, finalized, end_reason FROM listens",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(listened_ms, 123_000);
    assert_eq!(finalized, 1);
    assert_eq!(reason, "manual_next");
}

#[test]
fn semantic_event_insert_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    create_test_schema(&conn);
    write_listen(&conn, &sample_listen("listen-1", 1_700_000_000_000, true)).unwrap();
    let event = sample_event("event-1", "listen-1", 1_700_000_123_000);
    write_event(&conn, &event).unwrap();
    write_event(&conn, &event).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM playback_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn busy_writes_are_retried() {
    let mut attempts = 0;
    let value = retry_busy(|| {
        attempts += 1;
        if attempts < 3 {
            Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error {
                    code: rusqlite::ErrorCode::DatabaseBusy,
                    extended_code: 5,
                },
                None,
            ))
        } else {
            Ok("saved")
        }
    })
    .unwrap();
    assert_eq!(value, "saved");
    assert_eq!(attempts, 3);
}

#[test]
fn retention_keeps_newest_finalized_listens_and_cascades_events() {
    let conn = Connection::open_in_memory().unwrap();
    create_test_schema(&conn);
    for index in 1..=5 {
        let listen_id = format!("listen-{index}");
        write_listen(
            &conn,
            &sample_listen(&listen_id, 1_700_000_000_000 + index, true),
        )
        .unwrap();
        write_event(
            &conn,
            &sample_event(&format!("event-{index}"), &listen_id, index),
        )
        .unwrap();
    }

    assert_eq!(prune_listens_to(&conn, 3).unwrap(), 2);
    let timestamps = conn
        .prepare("SELECT started_at_ms FROM listens ORDER BY started_at_ms")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<i64>, _>>()
        .unwrap();
    assert_eq!(
        timestamps,
        vec![1_700_000_000_003, 1_700_000_000_004, 1_700_000_000_005]
    );
    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM playback_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(events, 3);
}

#[test]
fn shutdown_flushes_session_listen_and_event_before_joining() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "sparkle-db-writer-shutdown-{}-{unique}.db",
        std::process::id()
    ));
    {
        let conn = Connection::open(&path).unwrap();
        create_test_schema(&conn);
    }

    let writer = DbWriter::new(path.clone());
    let snapshot = SessionSnapshot {
        queue: vec![42],
        queue_index: Some(0),
        position_ms: 61_000,
        ..Default::default()
    };
    writer.save_session(snapshot);
    let mut listen = sample_listen("listen-42", 1_700_000_000_000, true);
    listen.track_id = 42;
    listen.listened_ms = 61_000;
    writer.upsert_listen(listen);
    let mut event = sample_event("event-42", "listen-42", 1_700_000_061_000);
    event.track_id = Some(42);
    writer.record_event(event);

    writer.shutdown().unwrap();

    let conn = Connection::open(&path).unwrap();
    let restored = crate::settings::load_session(&conn).unwrap();
    assert_eq!(restored.queue, vec![42]);
    assert_eq!(restored.position_ms, 61_000);
    let listened_ms: i64 = conn
        .query_row("SELECT listened_ms FROM listens", [], |row| row.get(0))
        .unwrap();
    assert_eq!(listened_ms, 61_000);
    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM playback_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(events, 1);
    drop(conn);

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}
