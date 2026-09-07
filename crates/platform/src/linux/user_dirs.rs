use std::fs;
use std::path::{Path, PathBuf};

use crate::{PlatformError, UserDirs};

pub(crate) fn resolve_user_dirs(
    home: &Path,
    config: &Path,
    data: PathBuf,
) -> Result<UserDirs, PlatformError> {
    let user_dirs = XdgUserDirs::from_file(&config.join("user-dirs.dirs"), home);
    UserDirs::new(
        personal_dir(user_dirs.desktop, home, &["Desktop", "桌面"]),
        personal_dir(
            user_dirs.documents,
            home,
            &["Documents", "文档", "文件", "文檔"],
        ),
        personal_dir(user_dirs.download, home, &["Downloads", "下载", "下載"]),
        personal_dir(user_dirs.pictures, home, &["Pictures", "图片", "圖片"]),
        personal_dir(user_dirs.videos, home, &["Videos", "视频", "影片", "視頻"]),
        personal_dir(user_dirs.music, home, &["Music", "音乐", "音樂"]),
        data,
    )
    .map_err(Into::into)
}

fn personal_dir(configured: Option<PathBuf>, home: &Path, names: &[&str]) -> PathBuf {
    // Explicit XDG paths (including $HOME for a disabled folder or an offline
    // mount) take precedence. Only infer a name when no valid setting exists.
    configured.unwrap_or_else(|| {
        names
            .iter()
            .map(|name| home.join(name))
            .find(|path| path.is_dir())
            .unwrap_or_else(|| home.join(names[0]))
    })
}

#[derive(Default)]
struct XdgUserDirs {
    desktop: Option<PathBuf>,
    documents: Option<PathBuf>,
    download: Option<PathBuf>,
    pictures: Option<PathBuf>,
    videos: Option<PathBuf>,
    music: Option<PathBuf>,
}

impl XdgUserDirs {
    fn from_file(path: &Path, home: &Path) -> Self {
        let Ok(contents) = fs::read_to_string(path) else {
            return Self::default();
        };
        let mut result = Self::default();
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, raw_value)) = line.split_once('=') else {
                continue;
            };
            let Some(value) = parse_user_dir_value(raw_value, home) else {
                continue;
            };
            match key.trim() {
                "XDG_DESKTOP_DIR" => result.desktop = Some(value),
                "XDG_DOCUMENTS_DIR" => result.documents = Some(value),
                "XDG_DOWNLOAD_DIR" => result.download = Some(value),
                "XDG_PICTURES_DIR" => result.pictures = Some(value),
                "XDG_VIDEOS_DIR" => result.videos = Some(value),
                "XDG_MUSIC_DIR" => result.music = Some(value),
                _ => {}
            }
        }
        result
    }
}

fn parse_user_dir_value(raw_value: &str, home: &Path) -> Option<PathBuf> {
    let encoded = raw_value.trim().strip_prefix('"')?;
    // Expand only a leading, unescaped HOME token, never arbitrary shell code.
    let (relative, encoded) = if encoded.starts_with("$HOME/") || encoded.starts_with("$HOME\"") {
        (true, &encoded[5..])
    } else if encoded.starts_with("${HOME}/") || encoded.starts_with("${HOME}\"") {
        (true, &encoded[7..])
    } else {
        (false, encoded)
    };
    let mut decoded = String::with_capacity(encoded.len());
    let mut characters = encoded.char_indices();
    while let Some((index, character)) = characters.next() {
        match character {
            '"' => {
                let trailing = encoded[index + 1..].trim();
                if !trailing.is_empty() && !trailing.starts_with('#') {
                    return None;
                }
                let value = if relative {
                    home.join(decoded.trim_start_matches('/'))
                } else {
                    PathBuf::from(decoded)
                };
                return value.is_absolute().then_some(value);
            }
            '\\' => {
                let (_, escaped) = characters.next()?;
                // Shell double quotes preserve backslashes before other characters.
                if !matches!(escaped, '\\' | '"' | '$' | '`') {
                    decoded.push('\\');
                }
                decoded.push(escaped);
            }
            // No variable/command substitution is evaluated, even in absolute paths.
            '$' | '`' => return None,
            _ => decoded.push(character),
        }
    }
    None
}

#[cfg(test)]
#[path = "user_dirs_tests.rs"]
mod tests;
