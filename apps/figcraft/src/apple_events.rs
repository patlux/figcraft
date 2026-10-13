//! macOS: files opened from Finder (a double-click, Open With, a drop on the Dock icon) or with
//! `open -a FigCraft file.figcraft`, and the quit Apple event.
//!
//! LaunchServices doesn't put these on the command line: it sends the running (or just-launched)
//! app a `kAEOpenDocuments` ('odoc') Apple event. winit 0.30 doesn't handle it and owns the
//! `NSApplicationDelegate`, so AppKit answered "FigCraft cannot open files in the “FigCraft
//! Document” format". Handling it ourselves needs Objective-C class declarations, i.e. `unsafe`,
//! which this workspace forbids. The audited `fmv-macos-events` crate (WordCraft, PhotoCraft and
//! PdfCraft use it too) registers an `NSAppleEventManager` handler before Finder's launch event,
//! leaves winit's delegate alone and hands the events over through a safe main-thread API.
//!
//! The crate takes over the quit event too (the Dock's Quit, logging out). It is answered at once
//! with `terminate:`, as AppKit's own handler did, not on a later frame: a window on another Space
//! draws no frames, and logging out would wait for it.

use std::cell::RefCell;

use dispatch2::DispatchQueue;
use figcraft_ui_egui::FigApp;
use fmv_macos_events::{Event, Inbox, Registration};
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;

/// What the events need, on the main thread (AppKit delivers them there).
#[derive(Default)]
struct State {
    inbox: Option<Inbox>,
    /// The UI to wake ([`connect`]).
    ctx: Option<egui::Context>,
    /// Files that arrived while the app couldn't take them (before the UI existed, or during a
    /// modal dialog's nested event loop).
    pending: Vec<String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Keeps the handlers registered; hold it until the event loop returns.
pub struct AppleEvents {
    _registration: Registration,
}

impl AppleEvents {
    /// Register the handlers. Call on the main thread before the event loop starts, so the event
    /// that launched the app (a Finder double-click) is caught too.
    pub fn install() -> Self {
        let (registration, inbox) = Registration::install();
        // The wake runs inside the event's handler, while the inbox is borrowed: act on the events
        // just after it, from the main queue.
        inbox.set_wake(|| DispatchQueue::main().exec_async(handle));
        STATE.with(|s| s.borrow_mut().inbox = Some(inbox));
        Self { _registration: registration }
    }
}

/// Wake `ctx`'s windows when files arrive; files that arrived before the UI existed open now.
pub fn connect(ctx: &egui::Context) {
    STATE.with(|s| s.borrow_mut().ctx = Some(ctx.clone()));
    handle();
}

/// Hand `app` the files still waiting (called every frame; there are usually none).
pub fn deliver(app: &mut FigApp, ctx: &egui::Context) {
    let paths = STATE.with(|s| std::mem::take(&mut s.borrow_mut().pending));
    if !paths.is_empty() {
        app.request_open(ctx, paths);
    }
}

/// Act on the events that arrived: quit at once, or hand the files to the app.
fn handle() {
    let events = STATE.with(|s| s.borrow().inbox.as_ref().map(Inbox::drain)).unwrap_or_default();
    let (paths, quit) = split(events);
    if quit {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).terminate(None);
        }
        return;
    }
    let ctx = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.pending.extend(paths);
        s.ctx.clone()
    });
    // Before the UI exists `connect` hands them over.
    let Some(ctx) = ctx else { return };
    // The app is busy only inside a frame (a modal dialog's nested event loop): the next one takes
    // them (`deliver`).
    if figcraft_ui_egui::windows::with_app(|app| deliver(app, &ctx)).is_none() {
        ctx.request_repaint();
    }
}

/// The files to open and whether to quit. `file.open` takes a path as text, so a path that isn't
/// UTF-8 is left out (with a warning) rather than mangled into another file's name.
fn split(events: Vec<Event>) -> (Vec<String>, bool) {
    let mut paths = Vec::new();
    let mut quit = false;
    for e in events {
        match e {
            Event::Open(files) => {
                for p in files {
                    match p.to_str() {
                        Some(s) => paths.push(s.to_owned()),
                        None => log::warn!("can't open {}: the path isn't UTF-8", p.display()),
                    }
                }
            }
            Event::Quit => quit = true,
        }
    }
    (paths, quit)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn files_open_in_order_and_quit_is_noticed() {
        let events = vec![
            Event::Open(vec![PathBuf::from("/tmp/a b.figcraft"), PathBuf::from(OsStr::from_bytes(b"/tmp/\xff.fig"))]),
            Event::Open(vec![PathBuf::from("/tmp/c.svg")]),
        ];
        assert_eq!(split(events), (vec!["/tmp/a b.figcraft".to_string(), "/tmp/c.svg".to_string()], false));
        assert!(split(vec![Event::Open(vec![PathBuf::from("/tmp/d.figcraft")]), Event::Quit]).1);
        assert_eq!(split(Vec::new()), (Vec::new(), false));
    }
}
