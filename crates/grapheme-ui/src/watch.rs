//! Watch one `.gr` file and report when its contents change.
//!
//! The desktop dev loop polls this from the window. Headless tests call
//! [`FileWatch::wait_change`] directly.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

/// Directory watch filtered to one file's contents.
pub struct FileWatch {
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<Event>>,
    path: PathBuf,
    last: String,
}

impl FileWatch {
    /// Watch `path`'s parent directory. The initial contents are the baseline.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let last = fs::read_to_string(path).unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })
        .map_err(std::io::Error::other)?;
        let dir = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(std::io::Error::other)?;
        Ok(Self {
            _watcher: watcher,
            rx,
            path: path.to_path_buf(),
            last,
        })
    }

    /// Block until the file contents differ, or `timeout` elapses.
    pub fn wait_change(&mut self, timeout: Duration) -> Option<String> {
        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            match self.rx.recv_timeout(deadline - now) {
                Ok(_) => {
                    thread::sleep(Duration::from_millis(5));
                    while self.rx.try_recv().is_ok() {}
                    let Ok(text) = fs::read_to_string(&self.path) else {
                        continue;
                    };
                    if text != self.last {
                        self.last = text.clone();
                        return Some(text);
                    }
                }
                Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_is_visible_within_300ms() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("grapheme-ui-watch-{unique}"));
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("app.gr");
        fs::write(&path, "state S { n: Int = 1 }\n").expect("seed");
        let mut watch = FileWatch::open(&path).expect("watch");
        let _ = watch.wait_change(Duration::from_millis(40));
        let started = Instant::now();
        fs::write(&path, "state S { n: Int = 2 }\n").expect("save");
        let got = watch.wait_change(Duration::from_millis(300));
        let elapsed = started.elapsed();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(got.as_deref(), Some("state S { n: Int = 2 }\n"));
        assert!(
            elapsed < Duration::from_millis(300),
            "file watch took {elapsed:?}"
        );
    }
}
