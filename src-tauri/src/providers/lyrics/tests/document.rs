use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Contract {
    name: String,
    text: String,
    lines: Vec<LrcLine>,
    plain: String,
    delay_ms: i64,
    shifted: String,
}

#[test]
fn shared_lrc_contract_matches_playback_editing_and_export() {
    let cases: Vec<Contract> =
        serde_json::from_str(include_str!("../../../../../test/fixtures/lrc.json")).unwrap();
    for case in cases {
        assert_eq!(parse_lrc(&case.text), case.lines, "{}: parse", case.name);
        assert_eq!(
            strip_lrc_timestamps(&case.text),
            case.plain,
            "{}: plain",
            case.name
        );
        assert_eq!(
            apply_lrc_offset(&case.text, case.delay_ms),
            case.shifted,
            "{}: bake",
            case.name
        );
        assert_eq!(
            apply_lrc_offset(&case.shifted, 0),
            case.shifted,
            "{}: no double shift",
            case.name
        );
    }
}

#[test]
fn translations_match_numeric_times_and_do_not_inject_headers() {
    assert_eq!(
        inject_translation(
            "[ar:Artist]\n[00:01.00]Hello",
            "[offset:100]\n[0:01.100]你好"
        ),
        "[00:01.000]Hello/你好"
    );
    assert!(parse_lrc("[999999999999999999999:00]Invalid").is_empty());
    assert_eq!(file_offset_ms("[offset:999999999999999999999]"), 0);
}
