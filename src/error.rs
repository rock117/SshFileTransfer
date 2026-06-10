use std::path::PathBuf;
use thiserror::Error;

/// Application error types
#[derive(Debug, Error)]
pub enum AppError {
    #[error("SSH connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("SFTP error: {0}")]
    SftpError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Remote file not found: {0}")]
    FileNotFound(String),

    #[error("Local path already exists: {0}")]
    PathExists(PathBuf),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Download interrupted: {0}")]
    Interrupted(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Key loading failed: {0}")]
    KeyLoadError(String),
}

pub type Result<T> = std::result::Result<T, AppError>;
