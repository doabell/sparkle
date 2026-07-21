use regex::Regex;

pub fn split_artists(input: &str, regex: &Regex, exceptions: &[String]) -> Vec<String> {
    let trimmed = input.trim();
    for exc in exceptions {
        if trimmed == exc.trim() {
            return vec![trimmed.to_string()];
        }
    }
    regex
        .split(input)
        .map(|s| normalize_artist_name(s))
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn normalize_artist_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}
