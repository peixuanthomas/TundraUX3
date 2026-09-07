use super::*;
use chrono::{Datelike, Timelike};

#[test]
fn parses_http_date_header_as_utc() {
    let parsed = parse_http_date("Tue, 15 Nov 1994 08:12:31 GMT").expect("date parses");

    assert_eq!(parsed.year(), 1994);
    assert_eq!(parsed.month(), 11);
    assert_eq!(parsed.day(), 15);
    assert_eq!(parsed.hour(), 8);
    assert_eq!(parsed.minute(), 12);
    assert_eq!(parsed.second(), 31);
}

#[test]
fn time_server_urls_require_http_and_are_canonicalized() {
    assert_eq!(
        normalize_time_server_url(" https://time.example.test ").unwrap(),
        "https://time.example.test/"
    );
    assert!(normalize_time_server_url("ntp://time.example.test").is_err());
    assert!(normalize_time_server_url("not a URL").is_err());
}
