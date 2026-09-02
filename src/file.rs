//! Shared locks for file paths.
//!
//! The model can emit several tool calls at once, and they run
//! concurrently. Two tools operating on the same file at the same time
//! can interleave their reads and writes, so every path gets a single
//! static lock, and a tool must hold it for the duration of its file
//! operations.
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};

use std::collections::HashMap;
use std::future::Future;
use std::path::{self, Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// A registry of path locks, one per file.
///
/// A lock is created the first time a path is touched and retained for
/// the lifetime of the process; each one is tiny, so the registry grows
/// at most with the number of files a session touches.
static LOCKS: LazyLock<Mutex<HashMap<PathBuf, &'static AsyncMutex<()>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Acquires the lock for `path`, waiting for any in-flight operation on
/// the same file to finish.
///
/// The returned guard holds the lock until it is dropped, so bind it to
/// a local that lives for the duration of the file operation, like
/// `let _lock = file::lock(&path).await;`.
pub fn lock(path: impl AsRef<Path>) -> impl Future<Output = MutexGuard<'static, ()>> {
    let path = normalize(path.as_ref());

    async move {
        let lock = *LOCKS
            .lock()
            .expect("path lock registry is poisoned")
            .entry(path)
            .or_insert_with(|| Box::leak(Box::new(AsyncMutex::new(()))));

        lock.lock().await
    }
}

/// Lexically folds `.` and `..` segments, so that equivalent paths (like
/// `./src/main.rs` and `src/main.rs`) share the same lock.
fn normalize(path: &Path) -> PathBuf {
    let mut folded = PathBuf::new();

    for component in path.components() {
        match component {
            path::Component::CurDir => {}
            path::Component::ParentDir if folded.pop() => {}
            component => folded.push(component),
        }
    }

    folded
}

#[cfg(test)]
mod tests {
    use super::{lock, normalize};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn normalizes_equivalent_paths() {
        assert_eq!(
            normalize(Path::new("./src/main.rs")),
            PathBuf::from("src/main.rs")
        );
        assert_eq!(
            normalize(Path::new("src/../main.rs")),
            PathBuf::from("main.rs")
        );
        assert_eq!(
            normalize(Path::new("/src/./main.rs")),
            PathBuf::from("/src/main.rs")
        );
    }

    #[tokio::test]
    async fn serializes_operations_on_the_same_path() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let mut tasks = Vec::new();

        for _ in 0..4 {
            let active = active.clone();
            let peak = peak.clone();

            tasks.push(tokio::spawn(async move {
                for _ in 0..1000 {
                    let _lock = lock(Path::new("src/main.rs")).await;

                    // The lock must keep at most one task inside this
                    // section at a time, even though we yield to give
                    // the other tasks a chance to run.
                    active.fetch_add(1, Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    peak.fetch_max(active.load(Ordering::SeqCst), Ordering::SeqCst);
                    active.fetch_sub(1, Ordering::SeqCst);
                }
            }));
        }

        for task in tasks {
            task.await.unwrap();
        }

        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn operations_on_different_paths_are_not_serialized() {
        let first = lock(Path::new("a.txt")).await;

        // A different path may be locked while the first lock is still
        // held.
        let second = lock(Path::new("b.txt")).await;

        drop(first);
        drop(second);
    }
}
