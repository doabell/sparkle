use super::*;

#[test]
fn extracts_current_line_markup() {
    let html = r#"
        <div class="line-jp">夢ならばどれほどよかったでしょう</div>
        <div class="line-ro">yume naraba</div>
        <div class="line-jp">未だにあなたのことを夢にみる</div>
    "#;
    assert_eq!(
        extract_lyrics(html).as_deref(),
        Some("夢ならばどれほどよかったでしょう\n未だにあなたのことを夢にみる")
    );
}

#[test]
fn extracts_current_search_ids() {
    let html = r#"
        <a href="/lyrics/108265/">current</a>
        <a href="/lyrics/108265/">duplicate</a>
    "#;
    let pattern = Regex::new(r#"href=["']/lyrics/(\d+)"#).unwrap();
    let ids: Vec<_> = pattern
        .captures_iter(html)
        .map(|capture| capture[1].to_string())
        .collect();
    assert_eq!(ids, vec!["108265", "108265"]);
}
