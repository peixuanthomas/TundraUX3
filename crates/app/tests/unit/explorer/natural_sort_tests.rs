use super::*;

#[test]
fn ascii_case_insensitive_sort_is_natural() {
    assert_eq!(
        natural_name_compare("FILE2.txt", "file10.TXT", false),
        Ordering::Less
    );
    assert_eq!(
        natural_name_compare("File02.txt", "file2.txt", false),
        Ordering::Greater
    );
    assert_eq!(
        natural_name_compare("README", "readme", false),
        Ordering::Equal
    );
}

#[test]
fn unicode_case_folding_keeps_natural_numeric_order() {
    assert_eq!(
        natural_name_compare("Ä2.txt", "ä10.txt", false),
        Ordering::Less
    );
}
