mod change_notifier;
mod client_state;
mod rust_analyzer_lsp;
mod utils;

pub(super) struct Stop;

use std::path::PathBuf;

pub use rust_analyzer_lsp::RustAnalyzerLsp;
pub use utils::*;

#[derive(Debug, Clone)]
pub enum LspNotification {
    Indexing { project: PathBuf, is_indexing: bool },
    IndexingProgress(IndexingProgress),
}

/// Tracks detailed indexing progress information
#[derive(Debug, Clone)]
pub struct IndexingProgress {
    /// Project being indexed
    pub project: PathBuf,
    
    /// Whether indexing is currently in progress
    pub is_indexing: bool,
    
    /// When indexing started (Some if indexing is in progress)
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    
    /// When indexing completed (Some if indexing finished)
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    
    /// Number of files estimated in project (pre-scan)
    pub estimated_files: Option<usize>,
    
    /// Number of crates detected in the project
    pub crate_count: Option<usize>,
    
    /// Current status message, if available
    pub status_message: Option<String>,
    
    /// Progress percentage (0-100), if available
    pub progress_percentage: Option<f32>,
}

impl IndexingProgress {
    /// Creates a new indexing progress tracker
    pub fn new(project: PathBuf) -> Self {
        Self {
            project,
            is_indexing: false,
            started_at: None,
            completed_at: None,
            estimated_files: None,
            crate_count: None,
            status_message: None,
            progress_percentage: None,
        }
    }
    
    /// Marks the start of indexing
    pub fn start_indexing(&mut self) {
        self.is_indexing = true;
        self.started_at = Some(chrono::Utc::now());
        self.completed_at = None;
    }
    
    /// Marks the completion of indexing
    pub fn complete_indexing(&mut self) {
        self.is_indexing = false;
        self.completed_at = Some(chrono::Utc::now());
        self.progress_percentage = Some(100.0);
    }
    
    /// Returns the elapsed time as a formatted string
    pub fn elapsed_time(&self) -> String {
        if let Some(start) = self.started_at {
            let now = match self.completed_at {
                Some(end) => end,
                None => chrono::Utc::now(),
            };
            
            let duration = now.signed_duration_since(start);
            let seconds = duration.num_seconds();
            
            if seconds < 60 {
                format!("{}s", seconds)
            } else if seconds < 3600 {
                format!("{}m {}s", seconds / 60, seconds % 60)
            } else {
                format!("{}h {}m {}s", seconds / 3600, (seconds % 3600) / 60, seconds % 60)
            }
        } else {
            "Not started".to_string()
        }
    }
    
    /// Gets a user-friendly status message
    pub fn status_message(&self) -> String {
        if !self.is_indexing && self.completed_at.is_some() {
            return format!("Indexing complete ({})", self.elapsed_time());
        }
        
        if let Some(msg) = &self.status_message {
            if let Some(percent) = self.progress_percentage {
                return format!("{} ({:.0}%) - {}", msg, percent, self.elapsed_time());
            }
            return format!("{} - {}", msg, self.elapsed_time());
        }
        
        if self.is_indexing {
            return format!("Indexing in progress - {}", self.elapsed_time());
        }
        
        "Ready".to_string()
    }
}

/// Container module for LspError
pub mod error {
    // ... existing code ...
}
