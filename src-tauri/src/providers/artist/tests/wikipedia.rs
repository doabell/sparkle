use super::*;

#[test]
fn percent_encode_ascii_and_spaces() {
    assert_eq!(percent_encode("Tyler, The Creator"), "Tyler%2C_The_Creator");
    assert_eq!(percent_encode("AC/DC"), "AC%2FDC");
}

#[test]
fn percent_encode_utf8() {
    // ö = U+00F6 = 0xC3 0xB6 in UTF-8
    assert_eq!(percent_encode("Björk"), "Bj%C3%B6rk");
    // 中 = U+4E2D = 0xE4 0xB8 0xAD
    assert_eq!(percent_encode("中"), "%E4%B8%AD");
}
