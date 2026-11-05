use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportIssue {
    pub path: String,
    pub message: String,
}

/// Partial success is explicit; scanning never modifies the input files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub added: usize,
    pub skipped: usize,
    pub ignored: usize,
    pub issues: Vec<ImportIssue>,
}
