use indicatif::HumanBytes;
use std::time::Instant;

/// Transfer statistics
#[derive(Debug, Default)]
pub struct TransferStats {
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub files_completed: usize,
    pub total_files: usize,
    pub start_time: Option<Instant>,
}

impl TransferStats {
    pub fn new() -> Self {
        Self {
            start_time: Some(Instant::now()),
            ..Default::default()
        }
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.start_time
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    pub fn bytes_per_sec(&self) -> f64 {
        let elapsed = self.elapsed_secs();
        if elapsed > 0.0 {
            self.transferred_bytes as f64 / elapsed
        } else {
            0.0
        }
    }
}

pub fn format_bytes(bytes: u64) -> String {
    HumanBytes(bytes).to_string()
}
