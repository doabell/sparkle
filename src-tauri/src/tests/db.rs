pub(crate) fn test_connection() -> rusqlite::Connection {
    sparkle_core::db::initialize_connection(rusqlite::Connection::open_in_memory().unwrap())
        .unwrap()
        .0
}
