use super::*;

#[test]
fn aes_cbc_encryption_matches_the_netease_wire_format() {
    assert_eq!(
        aes_encrypt(b"hello", NONCE.as_bytes(), IV.as_bytes()).unwrap(),
        "+J9Q3vLzLGFuqlWFQh3T3A=="
    );
}

#[test]
fn deserializes_netease_camel_case_song_count() {
    let response: SearchResponse =
        serde_json::from_str(r#"{"code":200,"result":{"songCount":1,"songs":[]}}"#).unwrap();
    assert_eq!(response.result.song_count, 1);
}
