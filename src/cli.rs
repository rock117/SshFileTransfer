use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use clap::{ArgAction, Parser};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// SSH/SFTP file download tool
#[derive(Parser, Debug)]
#[command(name = "sftp-download")]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Load parameters from a config file (auto-detected by extension: toml/json/yaml/yml).
    /// A missing file is silently ignored. CLI args always override config values.
    #[arg(long, value_name = "FILE", global = true)]
    pub config: Option<PathBuf>,

    /// SSH server hostname or IP address
    #[arg(short = 'H', long, env = "SSH_HOST")]
    pub host: Option<String>,

    /// SSH server port
    #[arg(short, long, env = "SSH_PORT")]
    pub port: Option<u16>,

    /// SSH username
    #[arg(short, long, env = "SSH_USER")]
    pub user: Option<String>,

    /// Password for authentication
    #[arg(short = 'P', long)]
    pub password: Option<String>,

    /// Private key file path for authentication
    #[arg(short, long)]
    pub key: Option<PathBuf>,

    /// Passphrase for encrypted private key
    #[arg(long)]
    pub key_passphrase: Option<String>,

    /// Connection timeout in seconds
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Remote file or directory path on the server
    #[arg(short, long)]
    pub remote: Option<String>,

    /// Local destination path
    #[arg(short, long)]
    pub local: Option<PathBuf>,

    /// Skip existing files (default: overwrite)
    #[arg(short, long, action = ArgAction::SetTrue)]
    pub skip: Option<bool>,

    /// Resume partial download (file only)
    #[arg(long, action = ArgAction::SetTrue)]
    pub resume: Option<bool>,

    /// Maximum parallel downloads for directory
    #[arg(short = 'j', long)]
    pub parallel: Option<usize>,

    /// Exclude files matching a glob pattern on the basename
    /// (repeatable, e.g. --exclude "*.log" --exclude "*~").
    /// Applied after --include: trims files that match any of these patterns.
    #[arg(short = 'x', long = "exclude", value_name = "PATTERN")]
    pub exclude: Option<Vec<String>>,

    /// Only download files matching a glob pattern on the basename
    /// (repeatable, e.g. --include "*.log" --include "*.txt").
    /// If non-empty, files not matching any pattern are skipped.
    #[arg(short = 'i', long = "include", value_name = "PATTERN")]
    pub include: Option<Vec<String>>,

    /// Case-insensitive matching for --include/--exclude patterns (default: case-sensitive)
    #[arg(long, action = ArgAction::SetTrue)]
    pub ignore_case: Option<bool>,

    /// Only download files modified on or after this date (e.g. "2026-06-01" or "2026-06-01 12:00:00")
    #[arg(long, value_name = "DATE")]
    pub since: Option<String>,

    /// Only download files modified on or before this date (e.g. "2026-06-29" or "2026-06-29 23:59:59")
    #[arg(long, value_name = "DATE")]
    pub until: Option<String>,

    /// Only download the N most recently modified files (mtime descending)
    #[arg(long, value_name = "N")]
    pub latest: Option<usize>,
}

/// Config file content. All fields optional to allow partial configs.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct FileConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub key: Option<PathBuf>,
    pub key_passphrase: Option<String>,
    pub timeout: Option<u64>,
    pub remote: Option<String>,
    pub local: Option<PathBuf>,
    pub skip: Option<bool>,
    pub resume: Option<bool>,
    pub parallel: Option<usize>,
    pub exclude: Option<Vec<String>>,
    pub include: Option<Vec<String>>,
    pub ignore_case: Option<bool>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub latest: Option<usize>,
}

impl FileConfig {
    /// Load and parse a config file based on its extension.
    pub fn load(path: &Path) -> anyhow::Result<FileConfig> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read config {}: {e}", path.display()))?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "toml" => Ok(toml::from_str(&content)?),
            "json" => Ok(serde_json::from_str(&content)?),
            "yaml" | "yml" => Ok(serde_yaml::from_str(&content)?),
            other => Err(anyhow::anyhow!(
                "unsupported config extension '.{other}' (expected: toml, json, yaml, yml)"
            )),
        }
    }
}

/// Fully resolved parameters after merging CLI, env, config file, and defaults.
#[derive(Debug)]
pub struct ResolvedArgs {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub key: Option<PathBuf>,
    pub key_passphrase: Option<String>,
    pub timeout: u64,
    pub remote: String,
    pub local: PathBuf,
    pub skip: bool,
    pub resume: bool,
    pub parallel: usize,
    pub exclude: Vec<String>,
    pub include: Vec<String>,
    pub ignore_case: bool,
    /// Lower-bound mtime (inclusive), as Unix seconds in UTC.
    pub since: Option<i64>,
    /// Upper-bound mtime (inclusive), as Unix seconds in UTC.
    pub until: Option<i64>,
    /// Keep only the N most recently modified files (applied after since/until).
    pub latest: Option<usize>,
}

impl ResolvedArgs {
    /// Render the resolved parameters as an equivalent command line.
    /// Secrets (password / key_passphrase) are masked for safe logging.
    pub fn to_command_line(&self) -> String {
        let mut parts: Vec<String> = vec!["sftp-download".to_string()];

        parts.push(format!("-H {}", shell_escape(&self.host)));
        parts.push(format!("-p {}", self.port));
        parts.push(format!("-u {}", shell_escape(&self.user)));

        if let Some(pw) = &self.password {
            parts.push(format!("-P {}", shell_escape(&mask_secret(pw))));
        }
        if let Some(key) = &self.key {
            parts.push(format!("-k {}", shell_escape(&key.to_string_lossy())));
        }
        if let Some(pass) = &self.key_passphrase {
            parts.push(format!("--key-passphrase {}", shell_escape(&mask_secret(pass))));
        }
        parts.push(format!("--timeout {}", self.timeout));
        parts.push(format!("-r {}", shell_escape(&self.remote)));
        parts.push(format!("-l {}", shell_escape(&self.local.to_string_lossy())));

        if self.skip {
            parts.push("-s".to_string());
        }
        if self.resume {
            parts.push("--resume".to_string());
        }
        parts.push(format!("-j {}", self.parallel));
        for p in &self.exclude {
            parts.push(format!("-x {}", shell_escape(p)));
        }
        for p in &self.include {
            parts.push(format!("-i {}", shell_escape(p)));
        }
        if self.ignore_case {
            parts.push("--ignore-case".to_string());
        }
        if let Some(s) = self.since {
            parts.push(format!("--since {}", shell_escape(&format_unix_as_date(s))));
        }
        if let Some(u) = self.until {
            parts.push(format!("--until {}", shell_escape(&format_unix_as_date(u))));
        }
        if let Some(n) = self.latest {
            parts.push(format!("--latest {n}"));
        }

        parts.join(" ")
    }
}

/// Mask a secret value for safe display. Shows length as asterisks,
/// capped at 8 chars so the real length isn't leaked for long secrets.
fn mask_secret(s: &str) -> String {
    let n = s.len().min(8);
    "*".repeat(n)
}

/// Minimal shell-style quoting: wrap in double quotes and escape embedded quotes/backslashes.
fn shell_escape(s: &str) -> String {
    if s.is_empty()
        || s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '\\' | '.' | '-' | '_' | ':' | '~'))
    {
        s.to_string()
    } else {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    }
}

/// Parse a user-supplied date/datetime string into a Unix timestamp (UTC seconds).
/// Accepted formats:
///   "YYYY-MM-DD"               -> start-of-day (00:00:00) for --since, end-of-day (23:59:59) for --until
///   "YYYY-MM-DD HH:MM:SS"      -> exact second
///   "YYYY-MM-DDTHH:MM:SS"      -> exact second (ISO 8601)
/// `end_of_day` controls how a bare date is interpreted:
///   true  -> 23:59:59 (used for --until, inclusive upper bound)
///   false -> 00:00:00 (used for --since, inclusive lower bound)
fn parse_date_arg(s: &str, end_of_day: bool) -> anyhow::Result<i64> {
    let s = s.trim();
    // Try full datetime first (space- or T-separated).
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
    {
        return Ok(dt.and_utc().timestamp());
    }
    // Fall back to date only.
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| anyhow::anyhow!("invalid date '{s}': {e}. Expected YYYY-MM-DD or YYYY-MM-DD HH:MM:SS"))?;
    let t = if end_of_day {
        NaiveTime::from_hms_opt(23, 59, 59).unwrap()
    } else {
        NaiveTime::from_hms_opt(0, 0, 0).unwrap()
    };
    Ok(d.and_time(t).and_utc().timestamp())
}

/// Render a Unix timestamp (UTC seconds) back to "YYYY-MM-DD HH:MM:SS" for logging.
fn format_unix_as_date(ts: i64) -> String {
    DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| ts.to_string())
}
/// Priority: CLI args > env vars > config file > built-in defaults.
pub fn parse_args() -> ResolvedArgs {
    let args = Args::parse();

    let cfg = match args.config.as_deref() {
        Some(p) if p.exists() => match FileConfig::load(p) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error: failed to load config '{}': {e}", p.display());
                std::process::exit(1);
            }
        },
        Some(p) => {
            eprintln!("Warning: config file '{}' not found, ignoring", p.display());
            FileConfig::default()
        }
        // No --config: use built-in defaults and CLI/env only.
        None => FileConfig::default(),
    };

    // Merge: CLI value wins over config value, then fall back to defaults.
    let host = args.host.or(cfg.host).unwrap_or_else(|| "localhost".to_string());
    let port = args.port.or(cfg.port).unwrap_or(22);
    let timeout = args.timeout.or(cfg.timeout).unwrap_or(30);
    let parallel = args.parallel.or(cfg.parallel).unwrap_or(4);
    let exclude = args.exclude.or(cfg.exclude).unwrap_or_default();
    let include = args.include.or(cfg.include).unwrap_or_default();
    let ignore_case = args.ignore_case.or(cfg.ignore_case).unwrap_or(false);
    let skip = args.skip.or(cfg.skip).unwrap_or(false);
    let resume = args.resume.or(cfg.resume).unwrap_or(false);

    let password = args.password.or(cfg.password);
    let key = args.key.or(cfg.key);
    let key_passphrase = args.key_passphrase.or(cfg.key_passphrase);

    // Time filters: parse since/until from CLI or config; missing => no filter.
    let since = match args.since.or(cfg.since) {
        Some(s) => match parse_date_arg(&s, false) {
            Ok(ts) => Some(ts),
            Err(e) => {
                eprintln!("Error: invalid --since: {e}");
                std::process::exit(2);
            }
        },
        None => None,
    };
    let until = match args.until.or(cfg.until) {
        Some(s) => match parse_date_arg(&s, true) {
            Ok(ts) => Some(ts),
            Err(e) => {
                eprintln!("Error: invalid --until: {e}");
                std::process::exit(2);
            }
        },
        None => None,
    };
    let latest = args.latest.or(cfg.latest);
    if let (Some(s), Some(u)) = (since, until) {
        if s > u {
            eprintln!("Error: --since is later than --until");
            std::process::exit(2);
        }
    }

    // Required fields
    let user = match args.user.or(cfg.user) {
        Some(u) => u,
        None => {
            eprintln!("Error: --user is required (via CLI, config file, or SSH_USER env)");
            std::process::exit(2);
        }
    };
    let remote = match args.remote.or(cfg.remote) {
        Some(r) => r,
        None => {
            eprintln!("Error: --remote is required (via CLI or config file)");
            std::process::exit(2);
        }
    };
    let local = match args.local.or(cfg.local) {
        Some(l) => l,
        None => {
            eprintln!("Error: --local is required (via CLI or config file)");
            std::process::exit(2);
        }
    };

    // Validate conflicts (previously enforced by clap attributes).
    if skip && resume {
        eprintln!("Error: --skip and --resume are mutually exclusive");
        std::process::exit(2);
    }
    if key_passphrase.is_some() && key.is_none() {
        eprintln!("Error: --key-passphrase requires --key");
        std::process::exit(2);
    }

    ResolvedArgs {
        host,
        port,
        user,
        password,
        key,
        key_passphrase,
        timeout,
        remote,
        local,
        skip,
        resume,
        parallel,
        exclude,
        include,
        ignore_case,
        since,
        until,
        latest,
    }
}
