use clap::Parser;
use sftp_download::{
    cli::Args,
    sftp_downloader::{DownloadOptions, SftpDownloader},
    ssh_client::{AuthMethod, SshClient, SshConfig},
};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .without_time()
        .with_target(false)
        .with_level(false)
        .init();

    let args = Args::parse();

    // Build auth method
    let auth = if let Some(key_path) = &args.key {
        AuthMethod::Key {
            private_key_path: key_path.to_string_lossy().to_string(),
            passphrase: args.key_passphrase.clone(),
        }
    } else if let Some(password) = &args.password {
        AuthMethod::Password(password.clone())
    } else {
        eprintln!("Error: Either --password or --key is required");
        std::process::exit(1);
    };

    // Build config
    let config = SshConfig::new(args.host, args.port, args.user, auth)
        .with_timeout(Duration::from_secs(args.timeout));

    // Connect to SSH server
    let mut client = SshClient::new(config);
    client.connect().await?;

    // Open SFTP session
    let sftp = client.open_sftp().await?;
    let downloader = SftpDownloader::new(sftp);

    // Auto-detect remote path type and download
    let stats = downloader.download_auto(&args.remote, &args.local, &DownloadOptions {
        skip_existing: args.skip,
        resume: args.resume,
        parallel: args.parallel,
    }).await?;

    // Single file summary (directory summary is printed in download_directory)
    if stats.total_files == 1 {
        let speed = stats.bytes_per_sec();
        println!("\nDownloaded {} in {:.2}s ({}/s)",
            sftp_download::progress::format_bytes(stats.transferred_bytes),
            stats.elapsed_secs(),
            sftp_download::progress::format_bytes(speed as u64)
        );
    }

    Ok(())
}
