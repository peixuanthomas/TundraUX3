use super::*;

impl ExplorerState {
    pub fn new(current_path: impl Into<PathBuf>, show_hidden: bool) -> Self {
        let config = ExplorerConfig {
            show_hidden,
            ..ExplorerConfig::default()
        };
        Self::with_config(current_path, &config)
    }

    pub fn with_config(current_path: impl Into<PathBuf>, config: &ExplorerConfig) -> Self {
        let current_path = current_path.into();
        Self {
            current_path: current_path.clone(),
            current_location: ExplorerLocation::Directory(current_path),
            all_entries: Vec::new(),
            entries: Vec::new(),
            selected_index: 0,
            selected_paths: BTreeSet::new(),
            selection_anchor: None,
            selection_cleared: false,
            query: String::new(),
            show_hidden: config.show_hidden,
            show_system: config.show_system,
            show_extensions: config.show_extensions,
            folders_first: config.folders_first,
            case_sensitive_sort: config.case_sensitive_sort,
            size_format: config.size_format,
            date_zone: config.date_zone,
            confirm_delete: config.confirm_delete,
            confirm_name_conflicts: config.confirm_name_conflicts,
            show_sidebar: config.show_sidebar,
            sort_field: config.sort_field.into(),
            sort_direction: config.sort_direction.into(),
            viewport_offset: 0,
            viewport_follows_focus: true,
            listing_warning_count: 0,
            back_history: Vec::new(),
            forward_history: Vec::new(),
            quick_locations: Vec::new(),
            clipboard: None,
            pending_dialog: None,
            pending_conflict: None,
            pending_restore: None,
            pending_transfer: None,
            drag: None,
            operation: None,
            message: None,
            error: None,
        }
    }

    pub fn selected_entry(&self) -> Option<&ExplorerEntry> {
        self.entries.get(self.selected_index)
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_entry().map(|entry| entry.path.clone())
    }

    /// The single selected item, which may differ from the keyboard focus.
    pub fn single_selected_entry(&self) -> Option<&ExplorerEntry> {
        let paths = self.effective_selected_paths();
        if paths.len() != 1 {
            return None;
        }
        self.entries.iter().find(|entry| entry.path == paths[0])
    }

    pub fn effective_selected_paths(&self) -> Vec<PathBuf> {
        if self.selection_cleared {
            Vec::new()
        } else if self.selected_paths.is_empty() {
            self.selected_path().into_iter().collect()
        } else {
            self.entries
                .iter()
                .filter(|entry| self.selected_paths.contains(&entry.path))
                .map(|entry| entry.path.clone())
                .collect()
        }
    }

    pub fn is_selected(&self, path: &Path) -> bool {
        if self.selection_cleared {
            false
        } else if self.selected_paths.is_empty() {
            self.selected_entry()
                .is_some_and(|entry| entry.path == path)
        } else {
            self.selected_paths.contains(path)
        }
    }

    pub fn select_index(&mut self, index: usize, mode: ExplorerSelectionMode) {
        if index >= self.entries.len() {
            return;
        }
        let path = self.entries[index].path.clone();
        match mode {
            ExplorerSelectionMode::Replace => {
                self.selected_paths.clear();
                self.selected_paths.insert(path.clone());
                self.selection_anchor = Some(path);
                self.selection_cleared = false;
            }
            ExplorerSelectionMode::Toggle => {
                if self.selected_paths.is_empty() && !self.selection_cleared {
                    self.selected_paths.extend(self.selected_path());
                }
                if !self.selected_paths.remove(&path) {
                    self.selected_paths.insert(path.clone());
                }
                self.selection_anchor = Some(path);
                self.selection_cleared = self.selected_paths.is_empty();
            }
            ExplorerSelectionMode::Range | ExplorerSelectionMode::AddRange => {
                let fallback_index = self.selected_index.min(self.entries.len() - 1);
                let anchor_index = self
                    .selection_anchor
                    .as_ref()
                    .and_then(|anchor| self.entries.iter().position(|entry| &entry.path == anchor))
                    .unwrap_or(fallback_index);
                if self.selection_anchor.is_none() {
                    self.selection_anchor = self
                        .entries
                        .get(anchor_index)
                        .map(|entry| entry.path.clone());
                }
                let (start, end) = if anchor_index <= index {
                    (anchor_index, index)
                } else {
                    (index, anchor_index)
                };
                if mode == ExplorerSelectionMode::Range {
                    self.selected_paths.clear();
                } else if self.selected_paths.is_empty() && !self.selection_cleared {
                    self.selected_paths.extend(self.selected_path());
                }
                self.selected_paths.extend(
                    self.entries[start..=end]
                        .iter()
                        .map(|entry| entry.path.clone()),
                );
                self.selection_cleared = false;
            }
            ExplorerSelectionMode::FocusOnly => {
                // Freeze the implicit selection before moving the focus.
                if self.selected_paths.is_empty() && !self.selection_cleared {
                    self.selected_paths.extend(self.selected_path());
                }
                if self.selection_anchor.is_none() {
                    self.selection_anchor = self.selected_path();
                }
            }
        }
        self.selected_index = index;
        self.viewport_follows_focus = true;
    }

    pub fn select_all(&mut self) {
        self.selected_paths = self
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        if let Some(entry) = self.selected_entry() {
            self.selection_anchor = Some(entry.path.clone());
        }
        self.selection_cleared = self.selected_paths.is_empty();
        self.viewport_follows_focus = true;
    }

    pub fn clear_selection(&mut self) {
        self.selected_paths.clear();
        self.selection_anchor = None;
        self.selection_cleared = true;
    }

    pub fn invert_selection(&mut self) {
        self.selected_paths = self
            .entries
            .iter()
            .filter(|entry| !self.is_selected(&entry.path))
            .map(|entry| entry.path.clone())
            .collect();
        self.selection_cleared = self.selected_paths.is_empty();
        self.selection_anchor = self.selected_path();
    }

    pub fn apply_projection(&mut self) {
        let focused_path = self.selected_entry().map(|entry| entry.path.clone());
        let query = self.query.to_lowercase();
        let mut entries = self
            .all_entries
            .iter()
            .filter(|entry| self.show_hidden || !entry.attributes.hidden)
            .filter(|entry| self.show_system || !entry.attributes.system)
            .filter(|entry| query.is_empty() || entry.name.to_lowercase().contains(&query))
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| compare_entries(self, left, right));
        self.entries = entries;
        let had_selected_paths = !self.selected_paths.is_empty();
        self.selected_paths
            .retain(|path| self.entries.iter().any(|entry| &entry.path == path));
        if had_selected_paths && self.selected_paths.is_empty() {
            self.selection_cleared = true;
        }
        if let Some(focused_path) = focused_path
            && let Some(index) = self
                .entries
                .iter()
                .position(|entry| entry.path == focused_path)
        {
            self.selected_index = index;
        }
        self.clamp_selection();
    }

    pub fn to_config(&self) -> ExplorerConfig {
        ExplorerConfig {
            show_hidden: self.show_hidden,
            show_system: self.show_system,
            show_extensions: self.show_extensions,
            folders_first: self.folders_first,
            case_sensitive_sort: self.case_sensitive_sort,
            size_format: self.size_format,
            date_zone: self.date_zone,
            confirm_delete: self.confirm_delete,
            confirm_name_conflicts: self.confirm_name_conflicts,
            show_sidebar: self.show_sidebar,
            sort_field: self.sort_field.into(),
            sort_direction: self.sort_direction.into(),
        }
    }

    pub(super) fn clamp_selection(&mut self) {
        if self.entries.is_empty() {
            self.selected_index = 0;
            self.viewport_offset = 0;
        } else if self.selected_index >= self.entries.len() {
            self.selected_index = self.entries.len() - 1;
        }
    }

    pub(super) fn set_success(&mut self, message: impl Into<LocalizedText>) {
        self.message = Some(message.into());
        self.error = None;
    }

    pub(super) fn set_error(&mut self, error: ExplorerError) {
        self.error = Some(error.localized().message.into());
        self.message = None;
    }
}
