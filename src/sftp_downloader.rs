use crate::error::{AppError, Result};
use crate::progress::{format_bytes, TransferStats};
use indicatif::{ProgressBar, ProgressStyle};
use russh_sftp::client::SftpSession;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Semaphore};

/// Download options
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub skip_existing: bool,
    pub resume: bool,
    pub parallel: usize,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            skip_existing: false,
            resume: false,
            parallel: 4,
        }
    }
}

/// File download result for ordered printing
struct FileResult {
    idx: usize,
    file_name: String,
    size: u64,
    transferred: u64,
    speed: Option<u64>,
}

/// Download task
#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub remote_path: String,
    pub local_path: std::path::PathBuf,
    pub file_size: u64,
}

/// SFTP downloader
pub struct SftpDownloader {
    sftp: Arc<SftpSession>,
}

impl SftpDownloader {
    pub fn new(sftp: SftpSession) -> Self {
        Self {
            sftp: Arc::new(sftp),
        }
    }

    /// Auto-detect remote path type and download
    pub async fn download_auto(
        &self,
        remote_path: &str,
        local_path: &Path,
        options: &DownloadOptions,
    ) -> Result<TransferStats> {
        let metadata = self
            .sftp
            .metadata(remote_path)
            .await
            .map_err(|e| AppError::FileNotFound(format!("{}: {}", remote_path, e)))?;

        if metadata.is_dir() {
            self.download_directory(remote_path, local_path, options).await
        } else {
            let bytes = self.download_file(remote_path, local_path, options).await?;
            Ok(TransferStats {
                total_files: 1,
                files_completed: 1,
                total_bytes: bytes,
                transferred_bytes: bytes,
                start_time: Some(std::time::Instant::now()),
            })
        }
    }

    /// Download a single file
    pub async fn download_file(
        &self,
        remote_path: &str,
        local_path: &Path,
        options: &DownloadOptions,
    ) -> Result<u64> {
        let file_name = Path::new(remote_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(remote_path);

        // Get remote file info
        let file_info = self
            .sftp
            .metadata(remote_path)
            .await
            .map_err(|e| AppError::FileNotFound(format!("{}: {}", remote_path, e)))?;

        let file_size = file_info.size.unwrap_or(0);

        // Check local file
        if local_path.exists() && options.skip_existing && !options.resume {
            let existing_size = local_path.metadata()?.len();
            if existing_size >= file_size {
                print_file_result(1, 1, file_name, file_size, existing_size, None);
                return Ok(existing_size);
            }
        }

        let start = Instant::now();

        // Create progress bar
        let progress = create_file_progress_bar(file_size);

        // Determine starting offset for resume
        let mut offset = if options.resume && local_path.exists() {
            let existing_size = local_path.metadata()?.len();
            if existing_size >= file_size {
                progress.finish_and_clear();
                print_file_result(1, 1, file_name, file_size, existing_size, None);
                return Ok(existing_size);
            }
            progress.inc(existing_size);
            existing_size
        } else {
            // Create parent directories
            if let Some(parent) = local_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            0
        };

        // Open remote file
        let mut remote_file = self
            .sftp
            .open(remote_path)
            .await
            .map_err(|e| AppError::SftpError(format!("Failed to open remote file: {}", e)))?;

        // Open/create local file
        let mut local_file = if options.resume && offset > 0 {
            tokio::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .open(local_path)
                .await?
        } else {
            tokio::fs::File::create(local_path).await?
        };

        // Download in chunks
        let chunk_size = 64 * 1024; // 64KB chunks
        let mut buffer = vec![0u8; chunk_size];

        // Seek to offset if resuming
        if offset > 0 {
            use tokio::io::AsyncSeekExt;
            remote_file.seek(std::io::SeekFrom::Start(offset)).await?;
        }

        while offset < file_size {
            let remaining = file_size - offset;
            let read_size = std::cmp::min(chunk_size as u64, remaining) as usize;

            let bytes_read = remote_file
                .read(&mut buffer[..read_size])
                .await
                .map_err(|e| AppError::SftpError(format!("Read error: {}", e)))?;

            if bytes_read == 0 {
                break;
            }

            local_file.write_all(&buffer[..bytes_read]).await?;
            offset += bytes_read as u64;
            progress.inc(bytes_read as u64);

            // Periodically flush for resume safety
            if offset % (1024 * 1024) == 0 {
                local_file.flush().await?;
            }
        }

        local_file.flush().await?;
        progress.finish_and_clear();

        // Calculate speed
        let elapsed = start.elapsed().as_secs_f64();
        let speed = if elapsed > 0.0 {
            Some((offset as f64 / elapsed) as u64)
        } else {
            None
        };

        print_file_result(1, 1, file_name, file_size, offset, speed);

        Ok(offset)
    }

    /// Download a directory recursively
    pub async fn download_directory(
        &self,
        remote_dir: &str,
        local_dir: &Path,
        options: &DownloadOptions,
    ) -> Result<TransferStats> {
        let mut stats = TransferStats::new();

        // Collect all files to download
        let tasks = self.collect_tasks(remote_dir, local_dir).await?;
        stats.total_files = tasks.len();
        stats.total_bytes = tasks.iter().map(|t| t.file_size).sum();

        // Print summary
        println!("\n{} files, {}, parallel: {}", stats.total_files, format_bytes(stats.total_bytes), options.parallel);
        println!("{}", "-".repeat(60));

        if tasks.is_empty() {
            println!("No files to download.");
            return Ok(stats);
        }

        // Create local directories
        self.create_local_dirs(&tasks).await?;

        // Channel for ordered printing
        let (tx, rx) = mpsc::unbounded_channel::<FileResult>();
        let total_files = stats.total_files;
        let printer_handle = tokio::spawn(async move {
            printer_task(rx, total_files).await;
        });

        // Semaphore for parallel control
        let semaphore = Arc::new(Semaphore::new(options.parallel));
        let mut handles = Vec::new();

        for (idx, task) in tasks.into_iter().enumerate() {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let sftp = self.sftp.clone();
            let skip_existing = options.skip_existing;
            let tx = tx.clone();

            let handle = tokio::spawn(async move {
                let result = download_file_simple(&sftp, task, idx, skip_existing).await;
                drop(permit);
                match result {
                    Ok((bytes, file_result)) => {
                        let _ = tx.send(file_result);
                        Ok(bytes)
                    }
                    Err(e) => Err(e),
                }
            });
            handles.push(handle);
        }

        // Drop the original tx so printer_task exits when all spawned tasks finish
        drop(tx);

        // Wait for all downloads
        let mut errors = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(bytes)) => {
                    stats.transferred_bytes += bytes;
                    stats.files_completed += 1;
                }
                Ok(Err(e)) => {
                    errors.push(e.to_string());
                }
                Err(e) => {
                    errors.push(e.to_string());
                }
            }
        }

        // Wait for printer to flush all output
        let _ = printer_handle.await;

        // Print errors if any
        for err in &errors {
            println!("Error: {}", err);
        }

        // Print final summary
        let elapsed = stats.elapsed_secs();
        let speed = stats.bytes_per_sec();
        println!("{}", "-".repeat(60));
        println!(
            "Downloaded {}/{} files, {} in {:.2}s ({}/s)",
            stats.files_completed,
            stats.total_files,
            format_bytes(stats.transferred_bytes),
            elapsed,
            format_bytes(speed as u64)
        );

        Ok(stats)
    }

    /// Collect download tasks from remote directory
    async fn collect_tasks(
        &self,
        remote_dir: &str,
        local_dir: &Path,
    ) -> Result<Vec<DownloadTask>> {
        let mut tasks = Vec::new();
        // Normalize remote_dir: ensure it doesn't end with '/'
        let remote_dir = remote_dir.trim_end_matches('/').to_string();
        let mut dirs_to_visit = vec![remote_dir.clone()];

        while let Some(current_dir) = dirs_to_visit.pop() {
            let mut read_dir = match self.sftp.read_dir(&current_dir).await {
                Ok(rd) => rd,
                Err(e) => {
                    println!("Warning: Failed to read directory {}: {}", current_dir, e);
                    continue;
                }
            };

            // Iterate over directory entries
            while let Some(entry) = read_dir.next() {
                let file_name = entry.file_name();
                let stat = entry.metadata();

                // Skip . and ..
                if file_name == "." || file_name == ".." {
                    continue;
                }

                // Build full remote path
                let full_path = if current_dir.ends_with('/') {
                    format!("{}{}", current_dir, file_name)
                } else {
                    format!("{}/{}", current_dir, file_name)
                };

                // Calculate relative path from remote_dir
                let relative = if full_path.starts_with(&remote_dir) {
                    full_path[remote_dir.len()..].trim_start_matches('/').to_string()
                } else {
                    full_path.clone()
                };
                let local_path = local_dir.join(&relative);

                if stat.is_dir() {
                    dirs_to_visit.push(full_path);
                } else {
                    let file_size = stat.size.unwrap_or(0);
                    tasks.push(DownloadTask {
                        remote_path: full_path,
                        local_path,
                        file_size,
                    });
                }
            }
        }

        Ok(tasks)
    }

    /// Create local directories for download tasks
    async fn create_local_dirs(&self, tasks: &[DownloadTask]) -> Result<()> {
        let mut dirs = std::collections::HashSet::new();

        for task in tasks {
            if let Some(parent) = task.local_path.parent() {
                dirs.insert(parent.to_path_buf());
            }
        }

        for dir in dirs {
            tokio::fs::create_dir_all(&dir).await?;
        }

        Ok(())
    }
}

fn create_file_progress_bar(size: u64) -> ProgressBar {
    let pb = ProgressBar::new(size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb
}

/// Download file, return bytes transferred and result info for ordered printing
async fn download_file_simple(
    sftp: &SftpSession,
    task: DownloadTask,
    idx: usize,
    skip_existing: bool,
) -> Result<(u64, FileResult)> {
    let file_name = Path::new(&task.remote_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&task.remote_path)
        .to_string();

    // Check if file exists and should skip
    if skip_existing && task.local_path.exists() {
        let existing_size = task.local_path.metadata()?.len();
        if existing_size >= task.file_size {
            return Ok((existing_size, FileResult {
                idx,
                file_name,
                size: task.file_size,
                transferred: task.file_size,
                speed: None,
            }));
        }
    }

    let start = Instant::now();

    // Open remote file
    let mut remote_file = sftp
        .open(&task.remote_path)
        .await
        .map_err(|e| AppError::SftpError(format!("Failed to open '{}': {}", task.remote_path, e)))?;

    // Create local file
    let mut local_file = tokio::fs::File::create(&task.local_path).await?;

    // Download
    let chunk_size = 64 * 1024;
    let mut buffer = vec![0u8; chunk_size];
    let mut total_read: u64 = 0;

    loop {
        let bytes_read = remote_file
            .read(&mut buffer)
            .await
            .map_err(|e| AppError::SftpError(format!("Read error: {}", e)))?;

        if bytes_read == 0 {
            break;
        }

        local_file.write_all(&buffer[..bytes_read]).await?;
        total_read += bytes_read as u64;
    }

    local_file.flush().await?;

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        Some((total_read as f64 / elapsed) as u64)
    } else {
        None
    };

    Ok((total_read, FileResult {
        idx,
        file_name,
        size: task.file_size,
        transferred: total_read,
        speed,
    }))
}

/// Receive file results and print them in order
async fn printer_task(mut rx: mpsc::UnboundedReceiver<FileResult>, total_files: usize) {
    let mut next = 0usize;
    let mut pending: HashMap<usize, FileResult> = HashMap::new();

    while let Some(result) = rx.recv().await {
        pending.insert(result.idx, result);
        while let Some(info) = pending.remove(&next) {
            print_file_result(next + 1, total_files, &info.file_name, info.size, info.transferred, info.speed);
            next += 1;
        }
    }
}

/// Print file download result
fn print_file_result(current: usize, total: usize, name: &str, size: u64, transferred: u64, speed: Option<u64>) {
    let percent = if size > 0 { (transferred * 100 / size) as usize } else { 100 };
    let speed_str = speed.map(|s| format!("{}/s", format_bytes(s))).unwrap_or_else(|| "N/A".to_string());

    println!(
        "({}/{}) {:<30} {:>10}  {:>3}%  {}",
        current,
        total,
        truncate_str(name, 30),
        format_bytes(transferred),
        percent,
        speed_str
    );
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("...{}", &s[s.len().saturating_sub(max_len - 3)..])
    }
}
