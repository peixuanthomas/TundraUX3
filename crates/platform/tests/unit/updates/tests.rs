use super::*;
#[test]
fn package_ids_reject_missing_repository_and_terminal_controls() {
    let id = "tundraux3;1.3.1-1;x86_64;updates";
    assert_eq!(PackageVersion::parse(id).unwrap().id(), id);
    assert!(PackageVersion::parse("tundraux3;1.3.1-1;x86_64;updates").is_ok());
    for id in [
        "tundraux3;1;x86_64;",
        "tundraux3;1;x86_64;repo\u{1b}[31m",
        "tundraux3;1;x86_64",
        "tundraux3;1;x86_64;repo;extra",
    ] {
        assert_eq!(
            PackageVersion::parse(id),
            Err(ServiceError::UntrustedTransaction)
        );
    }
    assert!(same_rpm_version("0:1.3.1-1", "1.3.1-1"));
    assert!(!same_rpm_version("1:1.3.1-1", "1.3.1-1"));
}
