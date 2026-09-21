use super::*;

#[test]
fn artist_values_split_only_on_nul_and_preserve_name_punctuation() {
    assert_eq!(
        split_artists(" AC/DC\0Tyler, The Creator\0Alice; Bob\0宇多田ヒカル\0"),
        vec!["AC/DC", "Tyler, The Creator", "Alice; Bob", "宇多田ヒカル"]
    );
    assert_eq!(
        split_artists(" Earth, Wind & Fire / Live "),
        vec!["Earth, Wind & Fire / Live"]
    );
    assert!(split_artists(" \0　 ").is_empty());
    assert_eq!(
        normalize_artist_name("  Alice\t\n Bob　Smith "),
        "Alice Bob Smith"
    );
}
