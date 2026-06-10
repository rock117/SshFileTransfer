pub mod cli;
pub mod error;
pub mod progress;
pub mod sftp_downloader;
pub mod ssh_client;

pub use cli::Args;
pub use error::{AppError, Result};
pub use progress::TransferStats;
pub use sftp_downloader::{DownloadOptions, DownloadTask, SftpDownloader};
pub use ssh_client::{AuthMethod, SshClient, SshConfig};
