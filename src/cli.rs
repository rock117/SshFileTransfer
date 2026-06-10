use clap::Parser;
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

    /// Remote file or directory path on the server
    #[arg(short, long)]
    pub remote: String,

    /// Local destination path
    #[arg(short, long)]
    pub local: PathBuf,

    /// Skip existing files (default: overwrite)
    #[arg(short, long)]
    pub skip: bool,

    /// Resume partial download (file only)
    #[arg(long, conflicts_with = "skip")]
    pub resume: bool,

    /// Maximum parallel downloads for directory
    #[arg(short = 'j', long, default_value_t = 4)]
    pub parallel: usize,
}

pub fn parse_args() -> Args {
    Args::parse()
}
