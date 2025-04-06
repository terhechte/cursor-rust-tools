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
}
