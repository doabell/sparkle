use super::*;

fn library_fixture() -> rusqlite::Connection {
    let conn = crate::db::test_connection();
    conn.execute_batch("INSERT INTO artists (id,name) VALUES (1,'Alice'),(2,'Bob');
        INSERT INTO albums (id,title) VALUES (1,'Album');
        INSERT INTO album_artists VALUES (1,1),(1,2);
        INSERT INTO tracks (id,file_path,title,album_id,genre,year,track_number,duration_ms,audio_format,sample_rate_hz,bit_depth,channels,file_size_bytes,embedded_lyrics) VALUES
        (1,'C:/Music/one.flac','Song',1,'Pop',2000,1,180000,'flac',96000,24,2,1000,'Words'),
        (2,'C:/Music/two.mp3',' song ',1,'Pop',2001,2,20000,'mp3',44100,NULL,1,500,NULL),
        (3,'C:/Else/three.bin',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL);
        UPDATE tracks SET audio_bitrate_kbps=128 WHERE id=2;
        INSERT INTO track_artists (track_id,artist_id,role) VALUES (1,1,'main'),(1,2,'main'),(2,2,'main');").unwrap();
    conn
}

fn playlist_members(conn: &rusqlite::Connection, id: i64) -> Vec<(i64, i64)> {
    conn.prepare(
        "SELECT track_id,position FROM playlist_tracks WHERE playlist_id=? ORDER BY position",
    )
    .unwrap()
    .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[test]
fn playlist_additions_are_ordered_idempotent_and_preserve_existing_positions() {
    let mut conn = library_fixture();
    let playlist =
        create_playlist_with_connection(&conn, "  Mine  ".into(), Some("Description".into()), None)
            .unwrap();
    assert_eq!(playlist.name, "Mine");
    assert_eq!(playlist.description.as_deref(), Some("Description"));
    assert_eq!(playlist.track_count, 0);
    add_tracks_to_playlist_with_connection(&mut conn, playlist.id, &[2, 1, 2]).unwrap();
    let original = playlist_members(&conn, playlist.id);
    assert_eq!(
        original.iter().map(|row| row.0).collect::<Vec<_>>(),
        vec![2, 1]
    );
    add_tracks_to_playlist_with_connection(&mut conn, playlist.id, &[1, 3, 2, 3]).unwrap();
    let members = playlist_members(&conn, playlist.id);
    assert_eq!(&members[..2], original.as_slice());
    assert_eq!(
        members.iter().map(|row| row.0).collect::<Vec<_>>(),
        vec![2, 1, 3]
    );
    assert!(members.windows(2).all(|rows| rows[0].1 < rows[1].1));
    add_tracks_to_playlist_with_connection(&mut conn, playlist.id, &[]).unwrap();
    add_tracks_to_playlist_with_connection(&mut conn, playlist.id, &[2, 1, 3]).unwrap();
    assert_eq!(playlist_members(&conn, playlist.id), members);
    assert_eq!(playlist_track_count(&conn, playlist.id, None).unwrap(), 3);
    assert_eq!(
        tracks_in_playlist(&conn, playlist.id)
            .unwrap()
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>(),
        vec![2, 1, 3]
    );
}

#[test]
fn a_failed_playlist_batch_rolls_back_and_the_connection_can_retry() {
    let mut conn = library_fixture();
    assert!(conn
        .pragma_query_value(None, "foreign_keys", |row| row.get::<_, bool>(0))
        .unwrap());
    let id = create_playlist_with_connection(&conn, "Mine".into(), None, None)
        .unwrap()
        .id;
    add_tracks_to_playlist_with_connection(&mut conn, id, &[2]).unwrap();
    let before = playlist_members(&conn, id);
    // The valid insertion before the invalid ID must roll back as well.
    assert!(add_tracks_to_playlist_with_connection(&mut conn, id, &[1, 999, 3]).is_err());
    assert_eq!(playlist_members(&conn, id), before);
    assert!(add_tracks_to_playlist_with_connection(&mut conn, 999, &[1]).is_err());
    assert!(playlist_members(&conn, 999).is_empty());
    add_tracks_to_playlist_with_connection(&mut conn, id, &[1, 3]).unwrap();
    assert_eq!(
        playlist_members(&conn, id)
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        vec![2, 1, 3]
    );
}

#[test]
fn playlist_edits_and_deletion_are_scoped_and_do_not_delete_library_tracks() {
    let mut conn = library_fixture();
    assert!(create_playlist_with_connection(&conn, " \n ".into(), None, None).is_err());
    let first = create_playlist_with_connection(&conn, "First".into(), None, None)
        .unwrap()
        .id;
    let second = create_playlist_with_connection(&conn, "Second".into(), None, None)
        .unwrap()
        .id;
    for id in [first, second] {
        add_tracks_to_playlist_with_connection(&mut conn, id, &[1, 2, 3]).unwrap();
    }
    assert!(
        update_playlist_with_connection(&conn, first, " ".into(), Some("Rejected".into())).is_err()
    );
    assert_eq!(
        conn.query_row("SELECT name FROM playlists WHERE id=?", [first], |r| r
            .get::<_, String>(
            0
        ))
        .unwrap(),
        "First"
    );
    update_playlist_with_connection(&conn, first, " Renamed ".into(), Some("Updated".into()))
        .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT name,description FROM playlists WHERE id=?",
            [first],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        ("Renamed".into(), "Updated".into())
    );
    remove_track_from_playlist_with_connection(&conn, first, 2).unwrap();
    remove_track_from_playlist_with_connection(&conn, first, 2).unwrap();
    assert_eq!(
        playlist_members(&conn, first)
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(playlist_members(&conn, second).len(), 3);
    delete_playlist_with_connection(&conn, first).unwrap();
    assert!(playlist_members(&conn, first).is_empty());
    assert_eq!(playlist_members(&conn, second).len(), 3);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn managed_playlists_reject_manual_membership_changes_and_live_mix_edits() {
    let mut conn = library_fixture();
    let folder =
        create_playlist_with_connection(&conn, "Folder".into(), None, Some("C:/Music/".into()))
            .unwrap();
    assert_eq!(folder.track_count, 2);
    assert!(
        add_tracks_to_playlist_with_connection(&mut conn, folder.id, &[3])
            .unwrap_err()
            .contains("managed")
    );
    assert!(playlist_members(&conn, folder.id).is_empty());
    refresh_live_mix_playlists_with_connection(&mut conn).unwrap();
    let live_id = conn
        .query_row(
            "SELECT id FROM playlists WHERE smart_query='mix:never_played'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    let before = playlist_members(&conn, live_id);
    assert!(!before.is_empty());
    assert!(add_tracks_to_playlist_with_connection(&mut conn, live_id, &[1]).is_err());
    assert!(remove_track_from_playlist_with_connection(&conn, live_id, before[0].0).is_err());
    assert!(update_playlist_with_connection(&conn, live_id, "Changed".into(), None).is_err());
    assert!(delete_playlist_with_connection(&conn, live_id).is_err());
    assert_eq!(playlist_members(&conn, live_id), before);
}

#[test]
fn library_health_counts_agree_with_every_drill_down() {
    let conn = library_fixture();
    let health = library_health_with_connection(&conn).unwrap();
    assert_eq!(health.track_count, 3);
    assert_eq!(health.album_count, 1);
    assert_eq!(health.artist_count, 2);
    assert_eq!(health.total_size_bytes, 1500);
    assert_eq!(health.unclassified_tracks, 1);
    assert_eq!(health.formats.iter().map(|f| f.tracks).sum::<i64>(), 3);
    for (kind, count, expected) in [
        ("titles", health.missing_titles, 1),
        ("artists", health.missing_artists, 1),
        ("albums", health.missing_albums, 1),
        ("genres", health.missing_genres, 1),
        ("lyrics", health.missing_lyrics, 2),
        ("years", health.missing_years, 1),
        ("track_numbers", health.missing_track_numbers, 1),
        ("duplicate_titles", health.duplicate_titles, 2),
        ("never_played", health.never_played, 3),
        ("lossless", health.lossless_tracks, 1),
        ("lossy", health.lossy_tracks, 1),
        ("high_resolution", health.high_resolution_tracks, 1),
        ("low_bitrate", health.low_bitrate_tracks, 1),
        ("audio_properties", health.missing_audio_properties, 1),
        ("durations", health.missing_durations, 1),
        ("very_short", health.very_short_tracks, 1),
        ("very_long", health.very_long_tracks, 0),
        ("mono", health.mono_tracks, 1),
    ] {
        assert_eq!(count, expected, "{kind}");
        assert_eq!(
            health_tracks_with_connection(&conn, kind).unwrap().len() as i64,
            count,
            "{kind}"
        );
    }
    assert!(health_tracks_with_connection(&conn, "not-a-category").is_err());
    let empty = library_health_with_connection(&crate::db::test_connection()).unwrap();
    assert_eq!(empty.track_count, 0);
    assert_eq!(empty.total_size_bytes, 0);
}

#[test]
fn search_preserves_artist_credits_and_finds_lyrics_on_untagged_tracks() {
    let conn = library_fixture();
    let results = search_with_connection(&conn, " Alice ").unwrap();
    assert_eq!(results.artists[0].name, "Alice");
    assert_eq!(results.tracks[0].artist_names, vec!["Alice", "Bob"]);
    let results = search_with_connection(&conn, "Album").unwrap();
    assert_eq!(results.albums[0].artist_ids, vec![1, 2]);
    assert_eq!(results.albums[0].artist_names, vec!["Alice", "Bob"]);
    assert_eq!(results.tracks.len(), 2);
    crate::cache::set_lyrics(&conn, 3, "custom", None, Some("Hidden needle line")).unwrap();
    crate::cache::set_lyrics(&conn, 3, "online", None, Some("Other needle line")).unwrap();
    let results = search_with_connection(&conn, "needle").unwrap();
    assert_eq!(results.lyric_tracks.len(), 1);
    assert_eq!(results.lyric_tracks[0].snippet, "Hidden needle line");
    assert_eq!(results.lyric_tracks[0].track.id, 3);
    let blank = search_with_connection(&conn, "  ").unwrap();
    assert!(
        blank.artists.is_empty()
            && blank.albums.is_empty()
            && blank.tracks.is_empty()
            && blank.lyric_tracks.is_empty()
    );
}

#[test]
fn search_treats_sql_wildcards_as_literal_in_every_metadata_field() {
    let conn = library_fixture();
    conn.execute("UPDATE artists SET name='100% Alice' WHERE id=1", [])
        .unwrap();
    conn.execute("UPDATE tracks SET genre='low_fi' WHERE id=2", [])
        .unwrap();
    let percent = search_with_connection(&conn, "%").unwrap();
    assert_eq!(percent.artists.len(), 1);
    assert_eq!(
        percent.tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![1]
    );
    let underscore = search_with_connection(&conn, "_").unwrap();
    assert_eq!(
        underscore.tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![2]
    );
}

#[test]
fn live_mix_refresh_is_idempotent_and_preserves_manual_playlists() {
    let mut conn = library_fixture();
    conn.execute("INSERT INTO playlists (id,name) VALUES (99,'Mine')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO playlist_tracks (playlist_id,track_id,position) VALUES (99,1,0)",
        [],
    )
    .unwrap();
    for _ in 0..2 {
        refresh_live_mix_playlists_with_connection(&mut conn).unwrap();
    }
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM playlists", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(tracks_in_playlist(&conn, 99).unwrap()[0].id, 1);
    assert_eq!(playlist_track_count(&conn, 99, None).unwrap(), 1);
    assert_eq!(
        playlist_track_count(&conn, 99, Some("C:/Music/")).unwrap(),
        2
    );
    assert_eq!(tracks_in_folder(&conn, "C:/Music/").unwrap().len(), 2);
    assert_eq!(live_mix_tracks(&conn, "recently_added").unwrap().len(), 3);
    assert_eq!(live_mix_tracks(&conn, "never_played").unwrap().len(), 3);
    assert!(live_mix_tracks(&conn, "most_played").unwrap().is_empty());
    assert!(live_mix_tracks(&conn, "unknown").is_err());
    let id = conn
        .query_row(
            "SELECT id FROM playlists WHERE smart_query='mix:never_played'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(
        playlist_track_count(&conn, id, Some("mix:never_played")).unwrap(),
        3
    );
    let discovery = discovery_tracks_with_connection(&conn).unwrap();
    assert_eq!(discovery.recently_added.len(), 3);
    assert_eq!(discovery.never_played.len(), 3);
    assert!(discovery.most_played.is_empty());
}

#[test]
fn lyric_snippets_are_unicode_safe_and_use_synced_fallback() {
    assert_eq!(
        lyric_snippet(None, Some("[00:01.00]Hello"), "hello"),
        "Hello"
    );
    assert_eq!(
        lyric_snippet(Some("First\nSecond"), None, "missing"),
        "First"
    );
    assert_eq!(lyric_snippet(None, None, "missing"), "");
    assert_eq!(
        lyric_snippet(Some(&"歌".repeat(200)), None, "歌")
            .chars()
            .count(),
        120
    );
}

#[test]
fn clearing_custom_lyrics_releases_the_custom_provider_override() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE tracks (id INTEGER PRIMARY KEY, lyrics_source TEXT); \
         CREATE TABLE lyrics (track_id INTEGER NOT NULL, source TEXT NOT NULL, PRIMARY KEY (track_id, source));",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tracks (id, lyrics_source) VALUES (1, 'custom')",
        [],
    )
    .unwrap();

    clear_custom_lyrics_record(&conn, 1).unwrap();

    let source: Option<String> = conn
        .query_row("SELECT lyrics_source FROM tracks WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(source, None);
}

#[test]
fn changing_lyrics_source_keeps_cached_provider_rows() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE tracks (id INTEGER PRIMARY KEY, lyrics_source TEXT); \
         CREATE TABLE lyrics (track_id INTEGER NOT NULL, source TEXT NOT NULL, PRIMARY KEY (track_id, source)); \
         INSERT INTO tracks (id) VALUES (1); \
         INSERT INTO lyrics (track_id, source) VALUES (1, 'lrclib'), (1, 'netease');",
    )
    .unwrap();

    set_track_lyrics_source_record(&conn, 1, Some("netease")).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM lyrics WHERE track_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let source: String = conn
        .query_row("SELECT lyrics_source FROM tracks WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(source, "netease");
}

#[test]
fn listening_stats_use_only_finalized_meaningful_listens() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE artists (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
         CREATE TABLE albums (id INTEGER PRIMARY KEY, title TEXT NOT NULL);
         CREATE TABLE tracks (
            id INTEGER PRIMARY KEY,
            title TEXT,
            album_id INTEGER,
            genre TEXT,
            year INTEGER
         );
         CREATE TABLE track_artists (
            track_id INTEGER NOT NULL,
            artist_id INTEGER NOT NULL,
            role TEXT NOT NULL
         );
         CREATE TABLE album_artists (
            album_id INTEGER NOT NULL,
            artist_id INTEGER NOT NULL
         );
         CREATE TABLE listens (
            track_id INTEGER NOT NULL,
            session_id TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL,
            listened_ms INTEGER NOT NULL,
            meaningful INTEGER NOT NULL,
            completed INTEGER NOT NULL,
            finalized INTEGER NOT NULL
         );
         INSERT INTO artists VALUES (1, 'Artist A'), (2, 'Artist B');
         INSERT INTO albums VALUES (1, 'Album A'), (2, 'Album B');
         INSERT INTO tracks VALUES
            (1, 'Track A', 1, 'Rock', 2000),
            (2, 'Track B', 2, 'Rock', 2010);
         INSERT INTO track_artists VALUES (1, 1, 'main'), (2, 2, 'main');
         INSERT INTO album_artists VALUES (1, 1), (2, 2);
         INSERT INTO listens VALUES
            (1, 'session-a', 1700000000000, 60000, 1, 1, 1),
            (2, 'session-a', 1700000060000, 45000, 1, 0, 1),
            (1, 'session-b', 1700000120000, 4000, 0, 0, 1),
            (1, 'session-c', 1700000180000, 90000, 1, 1, 0);",
    )
    .unwrap();

    let stats = listening_stats_with_connection(&conn, None).unwrap();
    assert_eq!(stats.total_plays, 2);
    assert_eq!(stats.total_ms, 105_000);
    assert_eq!(stats.unique_tracks, 2);
    assert_eq!(stats.unique_artists, 2);
    assert_eq!(stats.completed_plays, 1);
    assert_eq!(stats.discovery_tracks, 2);
    assert_eq!(stats.session_count, 1);
    assert_eq!(stats.top_tracks.len(), 2);
    assert_eq!(stats.top_artists.len(), 2);
    assert_eq!(stats.top_albums.len(), 2);
    assert_eq!(stats.top_genre.as_deref(), Some("Rock"));
    assert_eq!(stats.top_genre_ms, 105_000);
}
