use super::*;

#[test]
fn artist_splitting_preserves_unicode_order_and_named_exceptions() {
    let regex = Regex::new(r"[,;&/]").unwrap();
    assert_eq!(
        split_artists(" Alice ; Bob / 宇多田ヒカル ,, ", &regex, &[]),
        vec!["Alice", "Bob", "宇多田ヒカル"]
    );
    assert_eq!(
        split_artists("  AC/DC  ", &regex, &[" AC/DC ".into()]),
        vec!["AC/DC"]
    );
    assert_eq!(
        split_artists("AC/DC", &regex, &["ac/dc".into()]),
        vec!["AC", "DC"]
    );
    assert!(split_artists(" ; / ", &regex, &[]).is_empty());
    assert_eq!(
        normalize_artist_name("  Alice\t\n Bob　Smith "),
        "Alice Bob Smith"
    );
}
