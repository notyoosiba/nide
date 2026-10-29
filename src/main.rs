use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use nide::app::{App, Msg};
use nide::ui;

#[derive(Parser, Debug)]
#[command(name = "nide", about = "Terminal Workspace with CLI Agent Integration")]
struct Cli {
    /// Workspace root directory (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

fn setup_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        default_hook(info);
    }));
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root_dir = cli.path.canonicalize().unwrap_or(cli.path);

    setup_panic_hook();

    // Initialize terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create central MPSC channel
    let (tx, rx) = mpsc::channel::<Msg>();

    // Spawn event listener thread
    let tx_events = tx.clone();
    thread::spawn(move || {
        while let Ok(evt) = event::read() {
            let res = match evt {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        tx_events.send(Msg::Key(key))
                    } else {
                        Ok(())
                    }
                }
                Event::Mouse(mouse) => tx_events.send(Msg::Mouse(mouse)),
                Event::Paste(text) => tx_events.send(Msg::Paste(text)),
                Event::Resize(w, h) => tx_events.send(Msg::Resize(w, h)),
                _ => Ok(()),
            };
            if res.is_err() {
                break;
            }
        }
    });

    // Spawn signal listener thread (SIGINT, SIGTERM)
    let mut signals = Signals::new([SIGINT, SIGTERM])?;
    let signals_handle = signals.handle();
    let tx_sig = tx.clone();
    thread::spawn(move || {
        for _ in signals.forever() {
            if tx_sig.send(Msg::Quit).is_err() {
                break;
            }
        }
    });

    // Initialize application state
    let mut app = App::with_sender(root_dir, Some(tx.clone()));
    let _ = app.start_watcher();
    app.trigger_git_marks();

    // Main event loop
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        let timeout = if app.needs_parse() {
            Some(std::time::Duration::ZERO)
        } else if app.is_dirty() && app.middle_mode != nide::MiddleMode::Conflict {
            if let Some(last_edit) = app.last_edit {
                Some(
                    app.autosave_delay
                        .saturating_sub(last_edit.elapsed())
                        .min(std::time::Duration::from_millis(100)),
                )
            } else {
                Some(std::time::Duration::from_millis(100))
            }
        } else {
            None
        };

        let msg_res = match timeout {
            Some(t) => rx.recv_timeout(t).map_err(|e| match e {
                mpsc::RecvTimeoutError::Timeout => None,
                mpsc::RecvTimeoutError::Disconnected => Some(()),
            }),
            None => rx.recv().map_err(|_| Some(())),
        };

        match msg_res {
            Ok(msg) => {
                app.handle_msg(msg);
                // Drain any additional pending messages before next redraw
                while let Ok(msg) = rx.try_recv() {
                    app.handle_msg(msg);
                    if app.should_quit {
                        break;
                    }
                }
            }
            Err(None) => {
                // Time-sliced parsing: 3ms idle slice
                app.parse_more(std::time::Duration::from_millis(3));
            }
            Err(Some(())) => break,
        }

        app.tick();
    }

    // Clean up signal handler
    signals_handle.close();

    // Clean up active kitty image if present
    if let Some(id) = app.active_image_id {
        nide::viewer::emit_kitty_delete(&mut app, id);
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

pub fn osc52_copy(text: &str) {
    nide::osc52_copy(text);
}
