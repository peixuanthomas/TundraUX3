use super::*;

impl EditorDocument {
    pub fn untitled(kind: DocumentKind) -> Self {
        let source = DocumentText::new(kind, String::new());
        Self {
            path: None,
            kind,
            metadata: TextMetadata::default(),
            saved_source: source.clone(),
            source,
        }
    }

    pub fn from_text(path: Option<PathBuf>, kind: DocumentKind, source: impl Into<String>) -> Self {
        let source = source.into();
        let metadata = TextMetadata::from_source(&source, false);
        let source = DocumentText::new(kind, source);
        Self {
            path,
            kind,
            metadata,
            saved_source: source.clone(),
            source,
        }
    }

    pub fn from_bytes(
        path: Option<PathBuf>,
        kind: DocumentKind,
        bytes: &[u8],
    ) -> Result<Self, DocumentDecodeError> {
        let (utf8_bom, body) = if bytes.starts_with(UTF8_BOM) {
            (true, &bytes[UTF8_BOM.len()..])
        } else {
            (false, bytes)
        };
        let source = std::str::from_utf8(body)
            .map_err(|error| DocumentDecodeError {
                valid_up_to: error.valid_up_to(),
            })?
            .to_owned();
        Ok(Self::from_decoded_text(path, kind, source, utf8_bom))
    }

    /// Decodes an owned UTF-8 buffer without copying its body for the common
    /// no-BOM case. This is intended for asynchronous file readers that can
    /// transfer ownership of their completed byte buffer to the editor.
    pub fn from_owned_bytes(
        path: Option<PathBuf>,
        kind: DocumentKind,
        mut bytes: Vec<u8>,
    ) -> Result<Self, DocumentDecodeError> {
        let utf8_bom = bytes.starts_with(UTF8_BOM);
        if utf8_bom {
            bytes.drain(..UTF8_BOM.len());
        }
        let source = String::from_utf8(bytes).map_err(|error| DocumentDecodeError {
            valid_up_to: error.utf8_error().valid_up_to(),
        })?;
        Ok(Self::from_decoded_text(path, kind, source, utf8_bom))
    }

    pub(super) fn from_decoded_text(
        path: Option<PathBuf>,
        kind: DocumentKind,
        source: String,
        utf8_bom: bool,
    ) -> Self {
        let metadata = TextMetadata::from_source(&source, utf8_bom);
        let source = DocumentText::new(kind, source);
        Self {
            path,
            kind,
            metadata,
            saved_source: source.clone(),
            source,
        }
    }

    pub fn open(path: impl Into<PathBuf>, bytes: &[u8]) -> Result<Self, DocumentDecodeError> {
        let path = path.into();
        let kind = DocumentKind::from_path(&path);
        Self::from_bytes(Some(path), kind, bytes)
    }

    pub fn open_owned(
        path: impl Into<PathBuf>,
        bytes: Vec<u8>,
    ) -> Result<Self, DocumentDecodeError> {
        let path = path.into();
        let kind = DocumentKind::from_path(&path);
        Self::from_owned_bytes(Some(path), kind, bytes)
    }

    pub fn source(&self) -> Cow<'_, str> {
        self.source.text()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let source_len = match &self.source {
            DocumentText::Contiguous(source) => source.len(),
            DocumentText::Rope { text, .. } => text.len_bytes(),
        };
        let mut bytes = Vec::with_capacity(source_len + usize::from(self.metadata.utf8_bom) * 3);
        if self.metadata.utf8_bom {
            bytes.extend_from_slice(UTF8_BOM);
        }
        match &self.source {
            DocumentText::Contiguous(source) => bytes.extend_from_slice(source.as_bytes()),
            DocumentText::Rope { text, .. } => {
                for chunk in text.chunks() {
                    bytes.extend_from_slice(chunk.as_bytes());
                }
            }
        }
        bytes
    }

    pub fn is_dirty(&self) -> bool {
        self.source != self.saved_source
    }

    pub fn mark_saved(&mut self, path: Option<PathBuf>) {
        if let Some(path) = path {
            self.kind = DocumentKind::from_path(&path);
            self.path = Some(path);
        }
        let storage_matches_kind = matches!(
            (&self.source, self.kind),
            (DocumentText::Contiguous(_), DocumentKind::Markdown)
                | (DocumentText::Rope { .. }, DocumentKind::PlainText)
        );
        if !storage_matches_kind {
            self.source = DocumentText::new(self.kind, self.source.text().into_owned());
        }
        self.saved_source = self.source.clone();
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| self.kind.default_file_name().to_owned())
    }

    pub fn line_count(&self) -> usize {
        match &self.source {
            DocumentText::Contiguous(source) => line_ranges(source).len(),
            DocumentText::Rope { text, .. } => text.len_lines(),
        }
    }

    pub fn word_count(&self) -> usize {
        match &self.source {
            DocumentText::Contiguous(source) => source.unicode_words().count(),
            DocumentText::Rope { word_count, .. } => *word_count,
        }
    }

    pub(super) fn restore_source(&mut self, source: String) {
        self.source = DocumentText::new(self.kind, source);
        let fallback = self.metadata.preferred_line_ending;
        let source = self.source();
        self.metadata =
            TextMetadata::from_source_with_fallback(&source, self.metadata.utf8_bom, fallback);
    }
}
