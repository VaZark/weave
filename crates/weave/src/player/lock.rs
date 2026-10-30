//! One player per library: an exclusive lock on `<db>.host.lock`, held for as long as a
//! player runs and released by the OS even if the process dies.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// `<db><suffix>` next to the database file.
pub fn sibling(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub fn lock_path(db: &Path) -> PathBuf {
    sibling(db, ".host.lock")
}

/// Held while a player runs.
pub struct HostLock(#[allow(dead_code)] File);

impl HostLock {
    /// `None` when another player holds the lock.
    pub fn acquire(db: &Path) -> std::io::Result<Option<HostLock>> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path(db))?;
        match file.try_lock() {
            Ok(()) => {
                file.set_len(0)?;
                write!(file, "{}", std::process::id())?;
                Ok(Some(HostLock(file)))
            }
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }
}

/// Whether a player currently holds the lock for this library.
pub fn is_running(db: &Path) -> bool {
    let Ok(file) = File::open(lock_path(db)) else {
        return false;
    };
    match file.try_lock_shared() {
        Ok(()) => false, // nobody holds the exclusive lock
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_player_can_hold_the_lock_and_it_is_released_on_drop() {
        let db = std::env::temp_dir().join(format!("weave-lock-{}.db", std::process::id()));
        assert!(!is_running(&db), "no lock file yet");
        let first = HostLock::acquire(&db)
            .unwrap()
            .expect("the first player gets the lock");
        assert!(is_running(&db));
        assert!(
            HostLock::acquire(&db).unwrap().is_none(),
            "a second player is refused"
        );
        drop(first);
        assert!(!is_running(&db));
        assert!(HostLock::acquire(&db).unwrap().is_some(), "free again");
        let _ = std::fs::remove_file(lock_path(&db));
    }

    #[test]
    fn the_lock_file_sits_next_to_the_database() {
        let db = Path::new("/data/weave/library.db");
        assert_eq!(lock_path(db), Path::new("/data/weave/library.db.host.lock"));
    }
}
