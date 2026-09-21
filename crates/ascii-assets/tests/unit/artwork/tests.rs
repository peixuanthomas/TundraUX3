use super::*;

#[test]
fn ascii_art_rejects_characters_with_non_unit_terminal_width() {
    for content in ["天气", "column\tcolumn", "\u{1b}[31m", "\u{7f}"] {
        let error = ensure_ascii_lines("example", "text art", &[content.to_string()])
            .expect_err("non-cell-width art must be rejected");

        assert!(matches!(error, AssetError::InvalidAsset { .. }));
        assert!(error.to_string().contains("printable ASCII"));
    }
}
