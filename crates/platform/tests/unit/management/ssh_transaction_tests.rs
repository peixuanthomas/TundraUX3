use super::*;
#[test]
fn transaction_ids_cannot_escape_private_directory() {
    assert!(valid_id("123-456"));
    for id in ["", "../123", "/123", "123/1", "--help", "abc", "1\n"] {
        assert!(!valid_id(id));
    }
}
#[test]
fn only_confirmed_or_restored_transactions_are_finished() {
    assert!(finished(State::Committed));
    assert!(finished(State::Restored));
    for state in [
        State::Prepared,
        State::Applying,
        State::AwaitingConfirmation,
        State::RestoreFailed,
    ] {
        assert!(!finished(state));
    }
}
