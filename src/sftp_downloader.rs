use crate::error::{AppError, Result};
use crate::progress::{format_bytes, TransferStats};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use russh_sftp::client::SftpSession;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

/// Download options
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub skip_existing: bool,
    pub resume: bool,
    pub parallel: usize,
    /// Glob patterns matched against file basename; any match -> excluded.
    pub exclude_patterns: Vec<String>,
    /// Glob patterns matched against file basename; if non-empty, only matches are kept.
    pub include_patterns: Vec<String>,
    /// Case-insensitive glob matching when true.
    pub ignore_case: bool,
    /// Lower-bound mtime (inclusive), Unix seconds UTC. None = no lower bound.
    pub since: Option<i64>,
    /// Upper-bound mtime (inclusive), Unix seconds UTC. None = no upper bound.
    pub until: Option<i64>,
    /// Keep only the N most recently modified files (applied after include/exclude/since/until).
    pub latest: Option<usize>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            skip_existing: false,
            resume: false,
            parallel: 4,
            exclude_patterns: Vec::new(),
            include_patterns: Vec::new(),
            ignore_case: false,
            since: None,
            until: None,
            latest: None,
        }
    }
}

/// Download task
#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub remote_path: String,
    pub local_path: std::path::PathBuf,
    pub file_size: u64,
    /// File mtime in Unix seconds, when known.
    pub mtime: Option<i64>,
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
            let file_name = Path::new(remote_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(remote_path);
            // Include (whitelist) applied first, then exclude (blacklist) trims it.
            if !options.include_patterns.is_empty()
                && !matches_any_glob(file_name, &options.include_patterns, options.ignore_case)
            {
                println!("Skipped (not in include patterns): {}", remote_path);
                return Ok(TransferStats::new());
            }
            if matches_any_glob(file_name, &options.exclude_patterns, options.ignore_case) {
                println!("Skipped (excluded): {}", remote_path);
                return Ok(TransferStats::new());
            }
            if let Some(mtime) = metadata.mtime.map(|v| v as i64) {
                if !mtime_in_range(mtime, options.since, options.until) {
                    println!("Skipped (out of date range): {}", remote_path);
                    return Ok(TransferStats::new());
                }
            }

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
        let tasks = self
            .collect_tasks(remote_dir, local_dir, options)
            .await?;
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

        let total_files = stats.total_files;

        // Active style: shows real-time progress bar
        let active_style = ProgressStyle::default_bar()
            .template("{prefix} [{bar:22.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec}")
            .unwrap()
            .progress_chars("=>-");

        // Done style: plain message replacing the bar in place
        let done_style = ProgressStyle::default_bar()
            .template("{msg}")
            .unwrap();

        let mp = MultiProgress::new();
        let start_order = Arc::new(AtomicUsize::new(0));
        let next_display = Arc::new(AtomicUsize::new(1));

        // Semaphore for parallel control
        let semaphore = Arc::new(Semaphore::new(options.parallel));
        let mut handles = Vec::new();

        for task in tasks.into_iter() {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let order = start_order.fetch_add(1, Ordering::Relaxed) + 1;
            let sftp = self.sftp.clone();
            let skip_existing = options.skip_existing;
            let mp = mp.clone();
            let active_style = active_style.clone();
            let done_style = done_style.clone();
            let next_display = next_display.clone();

            let handle = tokio::spawn(async move {
                let result = download_file_simple(
                    &sftp,
                    task,
                    order,
                    skip_existing,
                    &mp,
                    active_style,
                    done_style,
                    total_files,
                    next_display,
                )
                .await;
                drop(permit);
                result
            });
            handles.push(handle);
        }

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

        // Print errors if any
        for err in &errors {
            eprintln!("Error: {}", err);
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
        options: &DownloadOptions,
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
                    // Include (whitelist) applied first, then exclude (blacklist) trims it.
                    if !options.include_patterns.is_empty()
                        && !matches_any_glob(&file_name, &options.include_patterns, options.ignore_case)
                    {
                        continue;
                    }
                    if matches_any_glob(&file_name, &options.exclude_patterns, options.ignore_case) {
                        continue;
                    }
                    let file_size = stat.size.unwrap_or(0);
                    let mtime = stat.mtime.map(|v| v as i64);
                    // Apply time filter at collect time so latest-N sees only matching files.
                    if let Some(mt) = mtime {
                        if !mtime_in_range(mt, options.since, options.until) {
                            continue;
                        }
                    }
                    tasks.push(DownloadTask {
                        remote_path: full_path,
                        local_path,
                        file_size,
                        mtime,
                    });
                }
            }
        }

        // Apply --latest: keep only the N most recently modified files (mtime desc).
        if let Some(n) = options.latest {
            if n == 0 {
                tasks.clear();
            } else if tasks.len() > n {
                // Sort by mtime descending; files without mtime sink to the bottom.
                tasks.sort_by(|a, b| b.mtime.cmp(&a.mtime));
                tasks.truncate(n);
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

/// Wait until it is this task's turn to create a progress bar line
async fn wait_for_display_turn(order: usize, next_display: &AtomicUsize) {
    while next_display.load(Ordering::Acquire) != order {
        tokio::task::yield_now().await;
    }
}

/// Download file with real-time progress bar; display order follows start order
async fn download_file_simple(
    sftp: &SftpSession,
    task: DownloadTask,
    order: usize,
    skip_existing: bool,
    mp: &MultiProgress,
    active_style: ProgressStyle,
    done_style: ProgressStyle,
    total_files: usize,
    next_display: Arc<AtomicUsize>,
) -> Result<u64> {
    let file_name = Path::new(&task.remote_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&task.remote_path);
    let prefix = format!(
        "({}/{}) {:<28}",
        order,
        total_files,
        truncate_str(file_name, 28)
    );

    wait_for_display_turn(order, &next_display).await;
    let pb = mp.add(ProgressBar::new(task.file_size));
    next_display.fetch_add(1, Ordering::Release);
    pb.set_style(active_style);
    pb.set_prefix(prefix.clone());

    let finish = |transferred: u64, speed: Option<u64>| {
        let size = task.file_size;
        let percent = if size > 0 { transferred * 100 / size } else { 100 };
        let speed_str = speed
            .map(|s| format!("{}/s", format_bytes(s)))
            .unwrap_or_else(|| "-".to_string());
        format!(
            "{} {:>10}  {:>3}%  {}",
            prefix,
            format_bytes(transferred),
            percent,
            speed_str,
        )
    };

    // Skip if already exists
    if skip_existing && task.local_path.exists() {
        let existing_size = task.local_path.metadata()?.len();
        if existing_size >= task.file_size {
            pb.set_style(done_style);
            pb.finish_with_message(finish(task.file_size, None));
            return Ok(existing_size);
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
        pb.inc(bytes_read as u64);
    }

    local_file.flush().await?;

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        Some((total_read as f64 / elapsed) as u64)
    } else {
        None
    };

    pb.set_style(done_style);
    pb.finish_with_message(finish(total_read, speed));

    Ok(total_read)
}

fn format_file_result(
    current: usize,
    total: usize,
    name: &str,
    size: u64,
    transferred: u64,
    speed: Option<u64>,
) -> String {
    let percent = if size > 0 {
        (transferred * 100 / size) as usize
    } else {
        100
    };
    let speed_str = speed
        .map(|s| format!("{}/s", format_bytes(s)))
        .unwrap_or_else(|| "N/A".to_string());

    format!(
        "({}/{}) {:<30} {:>10}  {:>3}%  {}",
        current,
        total,
        truncate_str(name, 30),
        format_bytes(transferred),
        percent,
        speed_str
    )
}

/// Print file download result
fn print_file_result(current: usize, total: usize, name: &str, size: u64, transferred: u64, speed: Option<u64>) {
    println!("{}", format_file_result(current, total, name, size, transferred, speed));
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("...{}", &s[s.len().saturating_sub(max_len - 3)..])
    }
}

/// Test whether `name` matches any of the glob `patterns`.
/// Supports `*` (any sequence, including empty), `?` (single char), and `[...]` (char set).
/// `ignore_case` makes the match case-insensitive for ASCII letters.
fn matches_any_glob(name: &str, patterns: &[String], ignore_case: bool) -> bool {
    patterns
        .iter()
        .any(|p| glob_match(name, p, ignore_case))
}

/// Recursive glob matcher: pattern vs name.
/// Supports `*` (greedy, spans any chars including none), `?` (one char), `[abc]` / `[!abc]`.
fn glob_match(name: &str, pattern: &str, ignore_case: bool) -> bool {
    // ASCII-lowercase helper used for case-insensitive comparison.
    #[inline]
    fn norm(c: char, ignore_case: bool) -> char {
        if ignore_case {
            c.to_ascii_lowercase()
        } else {
            c
        }
    }

    let name_chars: Vec<char> = name.chars().map(|c| norm(c, ignore_case)).collect();
    let pattern_chars: Vec<char> = pattern.chars().map(|c| norm(c, ignore_case)).collect();
    glob_match_recursive(&name_chars, 0, &pattern_chars, 0)
}

/// Backtracking glob matcher. `*` is the only backtracking point.
fn glob_match_recursive(
    name: &[char],
    mut ni: usize,
    pattern: &[char],
    mut pi: usize,
) -> bool {
    while pi < pattern.len() {
        match pattern[pi] {
            '*' => {
                // Collapse consecutive '*' into one.
                while pi < pattern.len() && pattern[pi] == '*' {
                    pi += 1;
                }
                if pi == pattern.len() {
                    return true; // trailing '*' matches the rest
                }
                // Try every remaining position in name as the start of the next pattern segment.
                while ni <= name.len() {
                    if glob_match_recursive(name, ni, pattern, pi) {
                        return true;
                    }
                    if ni == name.len() {
                        break;
                    }
                    ni += 1;
                }
                return false;
            }
            '?' => {
                if ni >= name.len() {
                    return false;
                }
                ni += 1;
                pi += 1;
            }
            '[' => {
                if ni >= name.len() {
                    return false;
                }
                // Character class: [...] or [!...]
                let mut negate = false;
                let mut j = pi + 1;
                if j < pattern.len() && (pattern[j] == '!' || pattern[j] == '^') {
                    negate = true;
                    j += 1;
                }
                let mut matched = false;
                // Closing ']' as first char is treated literally.
                let mut first = true;
                while j < pattern.len() {
                    if pattern[j] == ']' && !first {
                        break;
                    }
                    first = false;
                    // Range a-z?
                    if j + 2 < pattern.len() && pattern[j + 1] == '-' && pattern[j + 2] != ']' {
                        let lo = pattern[j];
                        let hi = pattern[j + 2];
                        if name[ni] >= lo && name[ni] <= hi {
                            matched = true;
                        }
                        j += 3;
                    } else {
                        if pattern[j] == name[ni] {
                            matched = true;
                        }
                        j += 1;
                    }
                }
                if j >= pattern.len() {
                    // Unterminated '[': treat literally as '['.
                    if (name[ni] == '[') != negate {
                        ni += 1;
                        pi += 1;
                        continue;
                    }
                    return false;
                }
                if matched == negate {
                    return false;
                }
                ni += 1;
                pi = j + 1; // skip past ']'
            }
            c => {
                if ni >= name.len() || name[ni] != c {
                    return false;
                }
                ni += 1;
                pi += 1;
            }
        }
    }
    ni == name.len()
}

/// Check whether a file mtime (Unix seconds) falls within [since, until].
/// `None` bounds mean unbounded on that side. If `mtime` itself is unknown,
/// callers should decide their own policy (we keep the file by skipping this check).
fn mtime_in_range(mtime: i64, since: Option<i64>, until: Option<i64>) -> bool {
    if let Some(s) = since {
        if mtime < s {
            return false;
        }
    }
    if let Some(u) = until {
        if mtime > u {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_basic_star() {
        assert!(glob_match("app.log", "*.log", false));
        assert!(glob_match("app.log", "*", false));
        assert!(!glob_match("app.log", "*.txt", false));
        // '*' matches empty sequence.
        assert!(glob_match("app", "app*", false));
        assert!(glob_match("app", "*app", false));
        assert!(glob_match("app", "a*p", false));
    }

    #[test]
    fn glob_question_mark() {
        assert!(glob_match("app.log", "app.log", false));
        assert!(glob_match("app.log", "??p.log", false));
        assert!(!glob_match("app.log", "?.log", false));
        assert!(glob_match("a", "?", false));
        assert!(!glob_match("", "?", false));
    }

    #[test]
    fn glob_char_class() {
        assert!(glob_match("app.log", "app.[lj]og", false));
        assert!(!glob_match("app.kog", "app.[lj]og", false));
        // Range.
        assert!(glob_match("app.log", "app.[a-z]og", false));
        assert!(!glob_match("app.Log", "app.[a-z]og", false));
        // Negated class.
        assert!(glob_match("app.kog", "app.[!lj]og", false));
        assert!(!glob_match("app.log", "app.[!lj]og", false));
    }

    #[test]
    fn glob_ignore_case() {
        assert!(!glob_match("app.LOG", "*.log", false));
        assert!(glob_match("app.LOG", "*.log", true));
        assert!(glob_match("App.Log", "app.log", true));
        assert!(glob_match("APP.LOG", "???.LOG", true));
        // `?` matches exactly one char.
        assert!(!glob_match("APP.LOG", "?.LOG", true));
    }

    #[test]
    fn glob_multiple_stars() {
        assert!(glob_match("dir/app.log", "*app.log", false)); // '*' spans 'dir/'
        assert!(glob_match("app.log.bak", "app*.bak", false));
        assert!(glob_match("app.log.bak", "*log*bak", false));
        assert!(!glob_match("app.log", "*log.tmp", false));
    }

    #[test]
    fn glob_no_extension_file() {
        assert!(glob_match("README", "README", false));
        assert!(glob_match("README", "*", false));
        assert!(!glob_match("README", "*.log", false));
        assert!(glob_match("README", "READ*", false));
    }

    #[test]
    fn glob_empty_pattern() {
        assert!(glob_match("", "", false));
        assert!(glob_match("", "*", false));
        assert!(!glob_match("a", "", false));
    }

    #[test]
    fn matches_any_glob_works() {
        let pats = vec!["*.log".to_string(), "*.txt".to_string()];
        assert!(matches_any_glob("app.log", &pats, false));
        assert!(matches_any_glob("app.txt", &pats, false));
        assert!(!matches_any_glob("app.tmp", &pats, false));
        // Empty patterns -> no match.
        assert!(!matches_any_glob("app.log", &[], false));
    }

    #[test]
    fn include_whitelist_semantics() {
        let include = vec!["*.log".to_string(), "*.txt".to_string()];
        // Matches one of the patterns -> kept.
        assert!(matches_any_glob("app.log", &include, false));
        assert!(matches_any_glob("notes.txt", &include, false));
        // No match -> would be skipped by whitelist.
        assert!(!matches_any_glob("app.tmp", &include, false));
        assert!(!matches_any_glob("README", &include, false));
        assert!(!matches_any_glob("archive.tar.gz", &include, false));
    }

    #[test]
    fn exclude_blacklist_semantics() {
        let exclude = vec!["*.tmp".to_string(), "*~".to_string()];
        // Matches -> would be excluded.
        assert!(matches_any_glob("build.tmp", &exclude, false));
        assert!(matches_any_glob("app.log~", &exclude, false));
        assert!(matches_any_glob("~", &exclude, false));
        // No match -> kept.
        assert!(!matches_any_glob("app.log", &exclude, false));
        assert!(!matches_any_glob("README", &exclude, false));
    }

    #[test]
    fn include_then_exclude_subset() {
        // Simulate the actual filter pipeline:
        //   include=*.log  -> {app.log, debug.log}
        //   exclude=debug* -> trims debug.log
        // Final: {app.log}
        let include = vec!["*.log".to_string()];
        let exclude = vec!["debug*".to_string()];

        // app.log: passes include, not excluded -> downloaded.
        assert!(matches_any_glob("app.log", &include, false));
        assert!(!matches_any_glob("app.log", &exclude, false));
        // debug.log: passes include, then excluded -> skipped.
        assert!(matches_any_glob("debug.log", &include, false));
        assert!(matches_any_glob("debug.log", &exclude, false));
        // app.txt: fails include -> skipped (exclude never consulted).
        assert!(!matches_any_glob("app.txt", &include, false));
    }

    #[test]
    fn ignore_case_affects_matching() {
        let pats = vec!["*.LOG".to_string()];
        // Case-sensitive: mismatched case -> no match.
        assert!(!matches_any_glob("app.log", &pats, false));
        assert!(matches_any_glob("app.LOG", &pats, false));
        // Case-insensitive: both match.
        assert!(matches_any_glob("app.log", &pats, true));
        assert!(matches_any_glob("app.LOG", &pats, true));
        assert!(matches_any_glob("App.Log", &pats, true));
    }

    #[test]
    fn empty_include_keeps_all() {
        // Empty include list -> matches_any_glob returns false,
        // which the caller interprets as "no whitelist active" (keep all).
        assert!(!matches_any_glob("anything.log", &[], false));
        assert!(!matches_any_glob("README", &[], false));
    }

    #[test]
    fn empty_exclude_excludes_none() {
        // Empty exclude list -> nothing matches -> nothing excluded.
        assert!(!matches_any_glob("anything.log", &[], false));
        assert!(!matches_any_glob("debug.log", &[], false));
    }

    #[test]
    fn no_extension_file_handling() {
        // No-extension files: only match patterns that don't require a dot ext.
        let include = vec!["*.log".to_string()];
        assert!(!matches_any_glob("README", &include, false)); // not whitelisted
        assert!(matches_any_glob("README", &["README".to_string()], false)); // exact match
        assert!(matches_any_glob("README", &["*".to_string()], false)); // wildcard
        assert!(matches_any_glob("README", &["READ*".to_string()], false)); // prefix
    }
}
