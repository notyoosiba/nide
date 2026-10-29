use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::app::Msg;

pub const DEBOUNCE_QUIET: Duration = Duration::from_millis(200);
pub const DEBOUNCE_MAX: Duration = Duration::from_millis(1000);

/// Check if a path should be ignored by the live watcher filter (e.g. `.nide-save-*` temp files).
#[must_use]
pub fn is_ignored_path(path: &Path) -> bool {
    for comp in path.components() {
        if let Some(name) = comp.as_os_str().to_str() {
            if name.starts_with(".nide-save-") {
                return true;
            }
        }
    }
    false
}

/// Convenience alias for `is_ignored_path`.
#[must_use]
pub fn should_ignore_path(path: &Path) -> bool {
    is_ignored_path(path)
}

/// Check if all paths in an event are ignored.
#[must_use]
pub fn should_ignore_event(event: &Event) -> bool {
    !event.paths.is_empty() && event.paths.iter().all(|p| is_ignored_path(p))
}

/// Filter an event, stripping any `.nide-save-*` paths. Returns `None` if no valid paths remain.
#[must_use]
pub fn filter_event(mut event: Event) -> Option<Event> {
    event.paths.retain(|p| !is_ignored_path(p));
    if event.paths.is_empty() {
        None
    } else {
        Some(event)
    }
}

/// Merge two events into one during debouncing, combining and deduplicating paths.
#[must_use]
pub fn merge_events(mut base: Event, next: Event) -> Event {
    base.paths.extend(next.paths);
    base.paths.sort();
    base.paths.dedup();
    base.kind = next.kind;
    base
}

/// Debounce loop that aggregates events within `quiet` duration, capped by `max_wait`.
pub fn debounce_loop(
    event_rx: mpsc::Receiver<Event>,
    out_tx: mpsc::Sender<Msg>,
    quiet: Duration,
    max_wait: Duration,
) {
    while let Ok(first_event) = event_rx.recv() {
        let first_instant = Instant::now();
        let mut last_instant = first_instant;
        let mut accumulated_event = first_event;

        loop {
            let elapsed_first = first_instant.elapsed();
            if elapsed_first >= max_wait {
                let _ = out_tx.send(Msg::Fs(accumulated_event));
                break;
            }

            let elapsed_last = last_instant.elapsed();
            if elapsed_last >= quiet {
                let _ = out_tx.send(Msg::Fs(accumulated_event));
                break;
            }

            let rem_quiet = quiet.saturating_sub(elapsed_last);
            let rem_max = max_wait.saturating_sub(elapsed_first);
            let wait_timeout = rem_quiet.min(rem_max);

            match event_rx.recv_timeout(wait_timeout) {
                Ok(next_event) => {
                    accumulated_event = merge_events(accumulated_event, next_event);
                    last_instant = Instant::now();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = out_tx.send(Msg::Fs(accumulated_event));
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = out_tx.send(Msg::Fs(accumulated_event));
                    return;
                }
            }
        }
    }
}

pub struct FsWatcher {
    watcher: RecommendedWatcher,
}

impl FsWatcher {
    pub fn watch_path(&mut self, path: &Path) -> notify::Result<()> {
        self.watcher.watch(path, RecursiveMode::NonRecursive)
    }
}

pub fn start_watcher(root: &Path, tx: mpsc::Sender<Msg>) -> notify::Result<FsWatcher> {
    start_watcher_with_debounce(root, tx, DEBOUNCE_QUIET, DEBOUNCE_MAX)
}

pub fn start_watcher_with_debounce(
    root: &Path,
    tx: mpsc::Sender<Msg>,
    quiet: Duration,
    max_wait: Duration,
) -> notify::Result<FsWatcher> {
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if let Some(filtered) = filter_event(event) {
                    let _ = event_tx.send(filtered);
                }
            }
        },
        Config::default(),
    )?;

    watcher.watch(root, RecursiveMode::Recursive)?;

    thread::Builder::new()
        .name("fs-debouncer".into())
        .spawn(move || {
            debounce_loop(event_rx, tx, quiet, max_wait);
        })
        .ok();

    Ok(FsWatcher { watcher })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_is_ignored_path() {
        assert!(is_ignored_path(Path::new(".nide-save-12345")));
        assert!(is_ignored_path(Path::new("/tmp/workspace/.nide-save-999")));
        assert!(is_ignored_path(Path::new("src/.nide-save-temp")));
        assert!(!is_ignored_path(Path::new("src/main.rs")));
        assert!(!is_ignored_path(Path::new(".gitignore")));
        assert!(!is_ignored_path(Path::new("nide-save-not-hidden")));
    }

    #[test]
    fn test_filter_event() {
        let event = Event {
            kind: notify::EventKind::Any,
            paths: vec![
                PathBuf::from("/tmp/.nide-save-123"),
                PathBuf::from("/tmp/doc.txt"),
            ],
            attrs: notify::event::EventAttributes::default(),
        };

        let filtered = filter_event(event).expect("some event");
        assert_eq!(filtered.paths, vec![PathBuf::from("/tmp/doc.txt")]);

        let ignored_event = Event {
            kind: notify::EventKind::Any,
            paths: vec![PathBuf::from("/tmp/.nide-save-456")],
            attrs: notify::event::EventAttributes::default(),
        };
        assert!(filter_event(ignored_event).is_none());
    }

    #[test]
    fn test_debounce_aggregates_events_within_quiet_window() {
        let (raw_tx, raw_rx) = mpsc::channel::<Event>();
        let (out_tx, out_rx) = mpsc::channel::<Msg>();

        let quiet = Duration::from_millis(40);
        let max_wait = Duration::from_millis(200);

        thread::spawn(move || {
            debounce_loop(raw_rx, out_tx, quiet, max_wait);
        });

        // Send first event
        raw_tx
            .send(Event {
                kind: notify::EventKind::Any,
                paths: vec![PathBuf::from("a.txt")],
                attrs: Default::default(),
            })
            .unwrap();

        // Send second event after 15ms (< 40ms quiet)
        thread::sleep(Duration::from_millis(15));
        raw_tx
            .send(Event {
                kind: notify::EventKind::Any,
                paths: vec![PathBuf::from("b.txt")],
                attrs: Default::default(),
            })
            .unwrap();

        // Wait for debounced message
        let msg = out_rx
            .recv_timeout(Duration::from_millis(150))
            .expect("should receive debounced event");

        if let Msg::Fs(event) = msg {
            assert_eq!(
                event.paths,
                vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")]
            );
        } else {
            panic!("Expected Msg::Fs");
        }
    }
}
