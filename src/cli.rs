use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// SSH/SFTP file download tool
#[derive(Parser, Debug)]
#[command(name = "sftp-download")]
#[command(version, about, long_about = None)]
pub struct Args {
    /// SSH server hostname or IP address
    #[arg(short = 'H', long, default_value = "localhost")]
    pub host: String,

    /// SSH server port
    #[arg(short, long, default_value_t = 22)]
    pub port: u16,

    /// SSH username
    #[arg(short, long, env = "SSH_USER")]
    pub user: String,

    /// Password for authentication
    #[arg(short = 'P', long)]
    pub password: Option<String>,

    /// Private key file path for authentication
    #[arg(short, long)]
    pub key: Option<PathBuf>,

    /// Passphrase for encrypted private key
    #[arg(long, requires = "key")]
    pub key_passphrase: Option<String>,

    /// Connection timeout in seconds
    #[arg(long, default_value_t = 30)]
    pub timeout: u64,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Download a single file
    DownloadFile {
        /// Remote file path on the server
        #[arg(short, long)]
        remote: String,

        /// Local destination path (file or directory)
        #[arg(short, long)]
        local: PathBuf,

        /// Overwrite existing files
        #[arg(short, long)]
        force: bool,

        /// Resume partial download
        #[arg(short = 'r', long)]
        resume: bool,
    },

    /// Download a directory recursively
    DownloadDir {
        /// Remote directory path on the server
        #[arg(short, long)]
        remote: String,

        /// Local destination directory
        #[arg(short, long)]
        local: PathBuf,

        /// Overwrite existing files
        #[arg(short, long)]
        force: bool,

        /// Maximum parallel downloads
        #[arg(short = 'p', long, default_value_t = 4)]
        parallel: usize,
    },
}

pub fn parse_args() -> Args {
    Args::parse()
}
