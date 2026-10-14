//! Several windows: a window holds its own tabs; closing it closes its files, asking first when
//! one has unsaved changes, and the main window takes over another window instead of quitting.

use std::sync::mpsc::{Receiver, channel};

use egui::{RawInput, ViewportEvent, ViewportId, ViewportInfo};
use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::control::{ControlReply, ControlRequest};
use crate::{FigApp, NoServices};

/// One main-window frame, as the desktop app runs it; `close` asks the window to close.
fn frame(ctx: &egui::Context, app: &mut FigApp, close: bool) {
    let mut input = RawInput::default();
    if close {
        input.viewports.insert(ViewportId::ROOT, ViewportInfo { events: vec![ViewportEvent::Close], ..Default::default() });
    }
    ctx.run_ui(input, |ui| {
        crate::windows::on_close_request(app, ui.ctx());
        app.ui(ui);
    })
    .textures_delta
    .clear();
}

/// A control request queued for the UI, and where its reply arrives.
fn request(app: &mut FigApp, method: &str, params: Value) -> Receiver<ControlReply> {
    let (reply, rx) = channel();
    app.control_queue.push(ControlRequest { id: json!(1), method: method.into(), params, reply, cancelled: Default::default() });
    rx
}

/// What eframe runs for the main window while it draws nothing: only the app's logic, no ui pass,
/// with each window's state (`hidden`: minimized or occluded, e.g. on another Space).
fn logic_only(ctx: &egui::Context, app: &mut FigApp, hidden: &[ViewportId]) {
    let mut input = RawInput::default();
    for id in std::iter::once(ViewportId::ROOT).chain(app.windows.list.iter().map(|w| crate::windows::viewport(w.id))) {
        let off = hidden.contains(&id);
        input.viewports.insert(id, ViewportInfo { minimized: Some(false), occluded: Some(off), ..Default::default() });
    }
    let _ = ctx.run_logic(&input, |ctx| crate::control::serve_hidden(app, ctx));
}

fn names(app: &FigApp) -> Vec<String> {
    app.session.docs.iter().map(|d| d.doc.name.clone()).collect()
}

#[test]
fn windows_hold_tabs_and_close_with_their_files() {
    let mut s = Session::empty();
    for n in ["A", "B", "C"] {
        s.execute("file.new", &json!({"name": n})).unwrap();
    }
    let ctx = egui::Context::default();
    // As eframe does where windows are native.
    ctx.set_embed_viewports(false);
    let mut app = FigApp::new(s, Box::new(NoServices));
    frame(&ctx, &mut app, false);
    let w = crate::control::handle(&mut app, &ctx, "window.new", &json!({"index": 0})).unwrap()["window"].as_u64().unwrap();
    frame(&ctx, &mut app, false);
    let list = crate::windows::list(&app);
    assert_eq!(list[0]["files"], json!(["B", "C"]));
    assert_eq!(list[1]["files"], json!(["A"]));
    assert_eq!(list[1]["id"], json!(w));
    assert!(app.session.ds().unwrap().doc.name != "A", "the main window shows its own files");
    assert!(crate::control::handle(&mut app, &ctx, "window.focus", &json!({"id": 99})).is_err());

    // Unsaved changes: the window stays and asks.
    let b = app.session.docs.iter().position(|d| d.doc.name == "B").unwrap();
    app.session.execute("doc.switch", &json!({"index": b})).unwrap();
    app.session.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
    frame(&ctx, &mut app, true);
    assert!(app.ui.confirm_close);
    assert_eq!(names(&app), ["A", "B", "C"]);

    // Saved (here: discarded) files close with the main window, which takes over the other one.
    app.ui.confirm_close = false;
    let b = app.session.docs.iter().position(|d| d.doc.name == "B").unwrap();
    app.session.execute("file.close", &json!({"index": b, "force": true})).unwrap();
    frame(&ctx, &mut app, true);
    frame(&ctx, &mut app, false);
    assert_eq!(names(&app), ["A"]);
    assert!(app.windows.list.is_empty());
    assert!(!app.ui.quit, "the app keeps running");
    assert_eq!(app.session.ds().unwrap().doc.name, "A");
}

#[test]
fn windows_that_draw_nothing_still_answer_control_requests() {
    let mut s = Session::empty();
    for n in ["A", "B"] {
        s.execute("file.new", &json!({"name": n})).unwrap();
    }
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut app = FigApp::new(s, Box::new(NoServices));
    frame(&ctx, &mut app, false);
    let w = crate::control::handle(&mut app, &ctx, "window.new", &json!({"index": 0})).unwrap()["window"].as_u64().unwrap();
    frame(&ctx, &mut app, false);
    let other = crate::windows::viewport(w);

    // The main window is on another Space: it still lists the windows and runs commands, in its
    // own file, without a frame.
    let list = request(&mut app, "window.list", json!({"window": 0}));
    let rect = request(
        &mut app,
        "engine.execute",
        json!({"window": 0, "command": "shape.rectangle", "params": {"x": 0, "y": 0, "width": 10, "height": 10}}),
    );
    logic_only(&ctx, &mut app, &[ViewportId::ROOT, other]);
    assert_eq!(list.try_recv().expect("window.list answered").result.unwrap()[1]["id"], json!(w));
    assert!(rect.try_recv().expect("engine.execute answered").result.is_ok());
    let b = app.session.docs.iter().find(|d| d.doc.name == "B").unwrap();
    assert!(b.is_dirty(), "the rectangle went into the main window's file");

    // Another window behind others works in its own file; the main window gets its file back.
    let other_rect =
        request(&mut app, "engine.execute", json!({"window": w, "command": "shape.rectangle", "params": {"x": 0, "y": 0, "width": 5, "height": 5}}));
    logic_only(&ctx, &mut app, &[other]);
    assert!(other_rect.try_recv().expect("the other window answered").result.is_ok());
    assert!(app.session.docs.iter().find(|d| d.doc.name == "A").unwrap().is_dirty(), "its own file changed");
    assert_eq!(app.session.ds().unwrap().doc.name, "B");
    assert_eq!(app.windows.current, 0);

    // A window screenshot needs a frame: refused at once instead of after the timeout.
    let shot = request(&mut app, "ui.screenshot", json!({"window": w, "path": "/tmp/never.png"}));
    logic_only(&ctx, &mut app, &[other]);
    assert!(shot.try_recv().expect("answered at once").result.unwrap_err().contains("visible"));

    // A window that draws keeps its requests for its frame.
    let later = request(&mut app, "window.list", json!({"window": 0}));
    logic_only(&ctx, &mut app, &[other]);
    assert!(later.try_recv().is_err(), "waits for the main window's frame");
    assert_eq!(app.control_queue.len(), 1);
    frame(&ctx, &mut app, false);
    assert!(later.try_recv().is_ok(), "answered in the frame");
}

#[test]
fn a_request_for_a_window_on_screen_wakes_it_while_the_main_window_is_hidden() {
    let mut s = Session::empty();
    for n in ["A", "B"] {
        s.execute("file.new", &json!({"name": n})).unwrap();
    }
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut app = FigApp::new(s, Box::new(NoServices));
    frame(&ctx, &mut app, false);
    let w = crate::control::handle(&mut app, &ctx, "window.new", &json!({"index": 0})).unwrap()["window"].as_u64().unwrap();
    frame(&ctx, &mut app, false);
    let other = crate::windows::viewport(w);
    // Which windows are asked to draw from now on (an idle window draws no frames until asked).
    let woken = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = woken.clone();
    ctx.set_request_repaint_callback(move |info| log.lock().unwrap().push(info.viewport_id));

    // Only the main window's logic runs while it is hidden: the request for the other window,
    // which is on screen, waits for that window's frame, and that window is asked to draw it.
    let ui = request(&mut app, "window.list", json!({"window": w}));
    logic_only(&ctx, &mut app, &[ViewportId::ROOT]);
    assert!(ui.try_recv().is_err(), "answered in the other window's frame");
    assert!(woken.lock().unwrap().contains(&other), "the other window is asked to draw: {:?}", woken.lock().unwrap());
}
