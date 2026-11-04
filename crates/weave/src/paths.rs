//! Where a player keeps its files by default.

use std::path::PathBuf;

/// The directory holding the library databases: `$XDG_DATA_HOME/weave`, else
/// `~/.local/share/weave`.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("weave")
}

/// The default library database, `library.db` in [`data_dir`].
pub fn default_db() -> PathBuf {
    data_dir().join("library.db")
}
