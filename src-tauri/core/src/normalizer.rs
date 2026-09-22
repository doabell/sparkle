pub fn split_artists(input: &str) -> Vec<String> {
    input
        .split('\0')
        .map(normalize_artist_name)
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn normalize_artist_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "tests/normalizer.rs"]
mod tests;
