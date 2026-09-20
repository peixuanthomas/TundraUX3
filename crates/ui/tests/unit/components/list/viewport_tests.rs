use super::List;
#[test]
fn automatic_viewport_boundaries() {
    assert_eq!(List::automatic_viewport_start(5, 4), 2);
    assert_eq!(List::automatic_viewport_start(5, 0), 0);
    assert_eq!(List::automatic_viewport_start(3, 4), 0);
}
