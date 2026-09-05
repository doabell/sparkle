use super::*;

#[test]
fn settings_defaults_match_legacy_deserialization_and_storage() {
    let conn = crate::db::test_connection();
    let expected = serde_json::to_value(Settings::default()).unwrap();
    assert_eq!(
        serde_json::to_value(load_settings(&conn).unwrap()).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_str::<Settings>("{}").unwrap()).unwrap(),
        expected
    );
}

#[test]
fn settings_report_corrupt_storage_and_missing_schema() {
    let conn = crate::db::test_connection();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, '{')",
        [UI_FONT_KEY],
    )
    .unwrap();
    assert!(load_settings(&conn).is_err());
    let missing = Connection::open_in_memory().unwrap();
    assert!(load_settings(&missing).is_err());
    assert!(save_settings(&missing, &Settings::default()).is_err());
}

#[test]
fn legacy_lyric_provider_is_migrated_without_reordering_sources() {
    let conn = crate::db::test_connection();
    save_json(
        &conn,
        LYRICS_SOURCES_KEY,
        &vec!["embedded", "petitlyrics", "kashinavi", "qq"],
    )
    .unwrap();
    assert_eq!(
        load_lyrics_sources(&conn).unwrap(),
        vec!["embedded", "kashinavi", "qq"]
    );
    save_json(&conn, ACCENT_COLOR_KEY, &"not a color").unwrap();
    assert_eq!(
        load_settings(&conn).unwrap().accent_color,
        Settings::default().accent_color
    );
}

#[test]
fn settings_roundtrip() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )
    .unwrap();
    let mut settings = Settings::default();
    settings.monitored_folders.push("C:\\Music".to_string());
    settings.artist_split_regex = "foo".to_string();
    settings.accent_color = "FA243C".to_string();
    settings.accent_foreground_preference = AccentForegroundPreference::Light;
    settings.debug_logging_enabled = true;
    settings.sound_check_enabled = true;
    settings.theme_mode = ThemeMode::Dark;
    settings.discord_artwork_s3_endpoint = "https://s3.example.test".to_string();
    settings.discord_artwork_s3_bucket = "artwork".to_string();
    settings.discord_artwork_s3_access_key = "access".to_string();
    settings.discord_artwork_s3_secret_key = "secret".to_string();
    settings.discord_artwork_store = "s3".to_string();
    save_settings(&conn, &settings).unwrap();
    let loaded = load_settings(&conn).unwrap();
    assert_eq!(loaded.monitored_folders, settings.monitored_folders);
    assert_eq!(loaded.artist_split_regex, settings.artist_split_regex);
    assert!(loaded.sound_check_enabled);
    assert_eq!(
        loaded.artist_split_exceptions,
        settings.artist_split_exceptions
    );
    assert_eq!(loaded.scan_on_startup, settings.scan_on_startup);
    assert_eq!(loaded.ui_font, settings.ui_font);
    assert_eq!(loaded.lyrics_font, settings.lyrics_font);
    assert_eq!(loaded.reduce_motion, settings.reduce_motion);
    assert_eq!(loaded.theme_mode, settings.theme_mode);
    assert_eq!(loaded.brave_api_key, settings.brave_api_key);
    assert_eq!(loaded.accent_color, "#fa243c");
    assert_eq!(
        loaded.accent_foreground_preference,
        settings.accent_foreground_preference
    );
    assert_eq!(loaded.discord_enabled, settings.discord_enabled);
    assert_eq!(loaded.discord_app_id, settings.discord_app_id);
    assert_eq!(
        loaded.discord_catbox_user_hash,
        settings.discord_catbox_user_hash
    );
    assert_eq!(loaded.discord_artwork_store, settings.discord_artwork_store);
    assert_eq!(
        loaded.discord_artwork_s3_endpoint,
        settings.discord_artwork_s3_endpoint
    );
    assert_eq!(
        loaded.discord_artwork_s3_bucket,
        settings.discord_artwork_s3_bucket
    );
    assert_eq!(
        loaded.discord_artwork_s3_access_key,
        settings.discord_artwork_s3_access_key
    );
    assert_eq!(
        loaded.discord_artwork_s3_secret_key,
        settings.discord_artwork_s3_secret_key
    );
    assert_eq!(loaded.debug_logging_enabled, settings.debug_logging_enabled);
    assert_eq!(loaded.lyrics_sources, settings.lyrics_sources);
    assert_eq!(loaded.artist_info_sources, settings.artist_info_sources);
    assert_eq!(loaded.artist_image_sources, settings.artist_image_sources);
    assert_eq!(loaded.album_art_sources, settings.album_art_sources);
}

#[test]
fn theme_mode_defaults_and_overrides_roundtrip() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )
    .unwrap();

    assert_eq!(load_settings(&conn).unwrap().theme_mode, ThemeMode::System);
    let legacy: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(legacy.theme_mode, ThemeMode::System);

    for mode in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::System] {
        let settings = Settings {
            theme_mode: mode,
            ..Settings::default()
        };
        save_settings(&conn, &settings).unwrap();
        assert_eq!(load_settings(&conn).unwrap().theme_mode, mode);
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.theme_mode, mode);
    }
    save_json(&conn, THEME_MODE_KEY, &"unknown").unwrap();
    assert_eq!(load_settings(&conn).unwrap().theme_mode, ThemeMode::System);
}

#[test]
fn legacy_accent_settings_are_normalized_without_a_schema_migration() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )
    .unwrap();
    save_json(&conn, ACCENT_COLOR_KEY, &"FA243C").unwrap();
    save_json(&conn, ACCENT_FOREGROUND_PREFERENCE_KEY, &"unknown").unwrap();

    let loaded = load_settings(&conn).unwrap();
    assert_eq!(loaded.accent_color, "#fa243c");
    assert_eq!(
        loaded.accent_foreground_preference,
        AccentForegroundPreference::Auto
    );

    let stored: String = load_json(&conn, ACCENT_COLOR_KEY, String::new()).unwrap();
    assert_eq!(stored, "#fa243c");
}

#[test]
fn missing_accent_preference_defaults_to_automatic() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )
    .unwrap();

    let loaded = load_settings(&conn).unwrap();
    assert_eq!(
        loaded.accent_foreground_preference,
        AccentForegroundPreference::Auto
    );
    assert_eq!(
        serde_json::to_string(&AccentForegroundPreference::Light).unwrap(),
        "\"light\""
    );
    assert_eq!(
        serde_json::to_string(&AccentForegroundPreference::Dark).unwrap(),
        "\"dark\""
    );
}
