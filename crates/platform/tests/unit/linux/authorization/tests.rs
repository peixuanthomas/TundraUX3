use super::*;
#[test]
fn only_an_unresolved_challenge_can_use_fallback() {
    let details = HashMap::new();
    assert_eq!(decision(true, false, &details), Decision::Allowed);
    assert_eq!(decision(false, true, &details), Decision::Challenge);
    assert_eq!(decision(false, false, &details), Decision::Denied);
    assert_eq!(
        decision(
            false,
            true,
            &HashMap::from([("polkit.dismissed".into(), "true".into())])
        ),
        Decision::Cancelled
    );
}
