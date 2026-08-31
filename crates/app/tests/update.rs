use app::update::{GITHUB_OWNER, GITHUB_REPO, UPDATE_PROTOCOL_VERSION, current_build_identity};

#[test]
fn update_public_identity_and_repository_contract_is_stable() {
    let identity = current_build_identity();
    assert_eq!(identity.package_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(GITHUB_OWNER, "peixuanthomas");
    assert_eq!(GITHUB_REPO, "TundraUX3");
    assert_eq!(UPDATE_PROTOCOL_VERSION, 1);
}
