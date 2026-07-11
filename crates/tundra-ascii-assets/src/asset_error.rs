use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("failed to resolve current executable path: {source}")]
    CurrentExe {
        #[source]
        source: std::io::Error,
    },

    #[error("current executable path has no parent: {path}")]
    MissingCurrentExeParent { path: PathBuf },

    #[error("ASCII asset root does not exist: {path}")]
    MissingRoot { path: PathBuf },

    #[error("ASCII asset root is not a directory: {path}")]
    RootNotDirectory { path: PathBuf },

    #[error("missing ASCII asset {asset} at {path}")]
    MissingAsset { asset: String, path: PathBuf },

    #[error("failed to read ASCII asset {asset} at {path}: {source}")]
    ReadAsset {
        asset: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse TOML asset {asset} at {path}: {source}")]
    ParseToml {
        asset: String,
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("invalid ASCII asset {asset}: {message}")]
    InvalidAsset { asset: String, message: String },

    #[error("unknown ASCII asset {asset}")]
    UnknownAsset { asset: String },

    #[error("failed to copy ASCII assets from {from} to {destination}: {error}")]
    CopyAssets {
        from: PathBuf,
        destination: PathBuf,
        error: String,
    },

    #[error("failed to derive Cargo profile dir from OUT_DIR {out_dir}")]
    InvalidOutDir { out_dir: PathBuf },
}
