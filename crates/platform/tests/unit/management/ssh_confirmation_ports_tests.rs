use super::*;

#[test]
fn adding_an_editor_port_cannot_be_confirmed_through_the_retained_old_port() {
    assert_eq!(confirmation_ports(&[22, 2222], &[22]), vec![2222]);
    assert_eq!(
        confirmation_ports(&[22, 2222, 2223], &[22]),
        vec![2222, 2223]
    );
}

#[test]
fn authentication_edits_and_port_removal_require_a_retained_listener() {
    assert_eq!(confirmation_ports(&[22], &[22]), vec![22]);
    assert_eq!(confirmation_ports(&[2222], &[22, 2222]), vec![2222]);
    assert_eq!(confirmation_ports(&[22], &[2222]), vec![22]);
}
