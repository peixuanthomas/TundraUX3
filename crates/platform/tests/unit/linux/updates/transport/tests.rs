use super::*;
#[test]
fn transaction_hints_use_the_packagekit_string_array_wire_contract() {
    let message = zbus::Message::method_call("/test", "SetHints")
        .unwrap()
        .build(&(transaction_hints(),))
        .unwrap();
    let (hints,): (Vec<String>,) = message.body().deserialize().unwrap();
    assert_eq!(hints, ["interactive=true", "background=false"]);
    assert_eq!(message.body().signature().to_string(), "as");
}

#[test]
fn history_requires_target_time_user_and_transaction_hint_together() {
    let record = Record {
        expected: PackageVersion::parse("tundraux3;2-1;x86_64;updates").unwrap(),
        started_ms: 1_700_000_000_000,
        uid: 1000,
        transaction: Some("/123_test".into()),
    };
    assert!(History::successful(&record).matches(&record));
    let mut history = History::successful(&record);
    history.uid += 1;
    assert!(!history.matches(&record));
    let mut history = History::successful(&record);
    history.path = "/other".into();
    assert!(!history.matches(&record));
    let mut history = History::successful(&record);
    history.time = "2020-01-01T00:00:00Z".into();
    assert!(!history.matches(&record));
    let mut history = History::successful(&record);
    history.succeeded = false;
    assert!(!history.matches(&record));
    let mut history = History::successful(&record);
    history.data = "tundraux3;3-1;x86_64;updates".into();
    assert!(!history.matches(&record));
}
