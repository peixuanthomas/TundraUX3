use super::*;

#[test]
fn default_editor_suffixes_cover_all_syntax_definitions() {
    for (pattern, _) in definitions::FILE_ASSOCIATIONS {
        let Some(extension) = pattern.strip_prefix("**/*.") else {
            continue;
        };
        let path = std::path::PathBuf::from(format!("document.{extension}"));
        assert!(crate::explorer::is_editor_document_path(&path), "{pattern}");
    }
}
