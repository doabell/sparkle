use super::*;

#[test]
fn dependency_upgrades_preserve_stored_audio_identities() {
    // Captured with Symphonia 0.5.5. Existing databases must still recognize
    // moved or renamed tracks after a parser or hashing dependency update.
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-support/fixtures");
    for (name, expected) in [
        (
            "tone.flac",
            "36166360c9a8e9641f749d22411c6510571f41b65dbbc944be1a60bed89b60e9",
        ),
        (
            "tone.mp3",
            "7b08ce7a793a0deacad2f7994f826dd8e521a5a65aba2c9847f5aa074f014cdd",
        ),
        (
            "tone.m4a",
            "7f3037d69674bdb2f1e06de87aca202a9b6690baa21bbf1ea8ff6e5ce1c860a5",
        ),
        (
            "tone.ogg",
            "fcc0b50890ee87136e0f7ec776b1e749050291a8da072f9ac756ce6c043f60d1",
        ),
        (
            "tone.aac",
            "7f3037d69674bdb2f1e06de87aca202a9b6690baa21bbf1ea8ff6e5ce1c860a5",
        ),
        (
            "tone-alac.m4a",
            "976f1592834b4aa5d0d642302aadc466a242f3a167eedb7a4b6515cfd7a6ecac",
        ),
        (
            "tone.opus",
            "02ae1417523c1a7096de8a4091da159c140a6abbf0cdeecb69893b8390e9b482",
        ),
    ] {
        assert_eq!(
            fingerprint(&fixtures.join(name)).unwrap(),
            format!("audio-v1:{expected}"),
            "{name}"
        );
    }
}
