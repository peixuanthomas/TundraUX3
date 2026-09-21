use super::*;
#[test]
fn system_hash_uses_fresh_salt_and_rejects_nul() {
    let first = password_hash("TestPassword123!").unwrap();
    let second = password_hash("TestPassword123!").unwrap();
    assert!(first.starts_with("$6$rounds=100000$"));
    assert_ne!(first, second);
    assert!(!first.contains("TestPassword"));
    assert!(password_hash("bad\0password").is_err());
    assert!(password_hash("").is_err());
}
