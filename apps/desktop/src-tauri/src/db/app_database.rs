//! Desktop database lifetime ownership, separate from generic pool creation.
use std::fs::File;
use std::path::Path;

use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};

/// Exclusive desktop database owner. Retain in application state for as long as
/// any worker can use the pool. The OS releases ownership even after a crash.
pub struct AppDatabase {
    pool: SqlitePool,
    _lock: File,
}

impl AppDatabase {
    /// Acquire ownership before migrations/recovery. A second desktop instance
    /// fails closed, while ordinary pool opens never interrupt live reviews.
    /// The persistent sidecar is never removed: unlinking a locked file would
    /// let another process lock a different inode. It contains no user data.
    /// # Errors
    /// Returns configuration error when already owned, or I/O/database errors.
    pub async fn open(path: &Path) -> AppResult<Self> {
        // Create without truncation, then resolve relative/symlink aliases before
        // deriving the sidecar. Do not lock SQLite's own file/byte ranges.
        drop(
            File::options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)?,
        );
        let path = path.canonicalize()?;
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".app.lock");
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => {
                AppError::Config("Database is already open in another desktop instance".into())
            }
            std::fs::TryLockError::Error(error) => AppError::Io(error),
        })?;
        let pool = super::init_pool_at(&path).await?;
        crate::repositories::review_repo::interrupt_active(&pool).await?;
        Ok(Self { pool, _lock: lock })
    }

    /// Pool for application workers. Keep this owner alive while clones are used.
    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
