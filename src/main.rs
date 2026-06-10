use clap::Parser;
use sftp_download::{
    cli::{Args, Commands},
    sftp_downloader::{DownloadOptions, SftpDownloader},
    ssh_client::{AuthMethod, SshClient, SshConfig},
};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging (debug level, no log level prefix)
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
        // Try to use password from env or prompt
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

    // Execute command
    match &args.command {
        Commands::DownloadFile {
            remote,
            local,
            force,
            resume,
        } => {
            let options = DownloadOptions {
                force: *force,
                resume: *resume,
                ..Default::default()
            };

            match downloader.download_file(remote, local, &options).await {
                Ok(bytes) => {
                    println!("Downloaded {} bytes", bytes);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::DownloadDir {
            remote,
            local,
            force,
            parallel,
        } => {
            let options = DownloadOptions {
                force: *force,
                resume: false,
                parallel: *parallel,
            };

            match downloader.download_directory(remote, local, &options).await {
                Ok(stats) => {
                    println!(
                        "Downloaded {}/{} files, {} bytes in {:.2}s",
                        stats.files_completed,
                        stats.total_files,
                        stats.transferred_bytes,
                        stats.elapsed_secs()
                    );
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}
