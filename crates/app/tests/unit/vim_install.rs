use super::*;

#[test]
fn cancellation_stops_before_network_or_filesystem_changes() {
    let cancel = UpdateCancellation::default();
    cancel.cancel();
    let error = install_from(
        "http://127.0.0.1:1/",
        "engine.zip",
        Path::new("unused"),
        &mut |_| panic!("cancelled install reported progress"),
        &cancel,
    )
    .unwrap_err();
    assert!(error.is_cancelled());
}
