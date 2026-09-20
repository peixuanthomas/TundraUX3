use super::*;
#[test]
fn process_binding_uses_start_time_even_with_parentheses_in_comm() {
    let fields = (3..=21).map(|_| "0").collect::<Vec<_>>().join(" ");
    assert_eq!(
        process_start_time(&format!("42 (shell (test)) {fields} 987 0")),
        Ok(987)
    );
    assert_eq!(
        process_start_time("42 (broken) 0"),
        Err(ServiceError::Unknown)
    );
    assert!(process_start_time(&std::fs::read_to_string("/proc/self/stat").unwrap()).is_ok());
}
