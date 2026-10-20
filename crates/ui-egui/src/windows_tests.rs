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

/// The desktop app's logic as eframe runs it for the main window, with each window's visibility
/// (`hidden`: minimised or covered). While the main window is hidden too this is all that runs.
fn logic(ctx: &egui::Context, app: &mut FigApp, hidden: &[ViewportId]) {
    let mut input = RawInput::default();
    for id in std::iter::once(ViewportId::ROOT).chain(app.windows.list.iter().map(|w| crate::windows::viewport(w.id))) {
        input.viewports.insert(id, ViewportInfo { minimized: Some(false), occluded: Some(hidden.contains(&id)), ..Default::default() });
    }
    let _ = ctx.run_logic(&input, |ctx| crate::control::drain_hidden(app, ctx));
}

fn names(app: &FigApp) -> Vec<String> {
    app.session.docs.iter().map(|d| d.doc.name.clone()).collect()
}

fn two_windows() -> (egui::Context, FigApp, u64) {
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
    (ctx, app, w)
}

#[test]
fn guarded_calls_reject_a_tab_switch_and_an_old_app_instance() {
    let (ctx, mut app, w) = two_windows();
    let inspected = crate::control::handle(&mut app, &ctx, "document.inspect", &json!({})).unwrap();
    let target = inspected["target"].clone();
    let other_index = app.session.docs.iter().position(|d| d.doc.name == "A").unwrap();
    app.session.execute("doc.switch", &json!({"index": other_index})).unwrap();
    let before = names(&app);
    let params = json!({"target": target, "command": "file.rename", "params": {"name": "Wrong"}});
    assert!(crate::control::handle(&mut app, &ctx, "engine.execute", &params).is_err(), "do not edit the new active tab");
    assert_eq!(names(&app), before);
    let mut fresh = crate::control::handle(&mut app, &ctx, "document.inspect", &json!({})).unwrap()["target"].clone();
    fresh["instance"] = json!("previous-app-run");
    assert!(
        crate::control::handle(&mut app, &ctx, "engine.execute", &json!({"target": fresh, "command": "file.rename", "params": {"name": "Wrong"}}))
            .is_err()
    );
    assert_eq!(names(&app), before);
    assert!(w > 0);
}

#[test]
fn guarded_calls_check_revision_and_target_after_queueing_even_when_hidden() {
    let (ctx, mut app, w) = two_windows();
    let target = crate::windows::list(&app)[0]["target"].clone();
    let queued = request(&mut app, "engine.execute", json!({"window": 0, "target": target, "command": "file.rename", "params": {"name": "Stale"}}));
    app.session.execute("file.rename", &json!({"name": "Newer edit"})).unwrap();
    let before = names(&app);
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    assert!(queued.try_recv().unwrap().result.unwrap_err().contains("revision changed"));
    assert_eq!(names(&app), before);
    let mut identity = crate::windows::list(&app)[0]["target"].clone();
    identity.as_object_mut().unwrap().remove("revision");
    let queued =
        request(&mut app, "engine.execute", json!({"window": 0, "target": identity, "command": "file.rename", "params": {"name": "Correct"}}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    assert!(queued.try_recv().unwrap().result.is_ok());
    assert_eq!(app.session.ds().unwrap().doc.name, "Correct");
    // Raw input waits for a frame: a guarded request is refused even for a window on screen.
    let target = crate::windows::list(&app)[0]["target"].clone();
    let raw = crate::control::handle(&mut app, &ctx, "ui.input", &json!({"target": target, "events": [{"text": "not deferred"}]}));
    assert!(raw.unwrap_err().contains("deferred"));
    assert!(crate::windows::take_input(&mut app, ViewportId::ROOT).is_empty());
}

#[test]
fn guarded_calls_reject_wrong_window_path_and_missing_identity() {
    let (ctx, mut app, w) = two_windows();
    let target = crate::windows::list(&app)[0]["target"].clone();
    let before = names(&app);
    for (key, value) in [("window", json!(w)), ("path", json!("/tmp/other-synthetic.figcraft")), ("document", Value::Null)] {
        let mut bad = target.clone();
        bad[key] = value;
        let r = request(&mut app, "engine.execute", json!({"window": 0, "target": bad, "command": "file.rename", "params": {"name": "Wrong"}}));
        logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
        assert!(r.try_recv().unwrap().result.is_err(), "{key}");
        assert_eq!(names(&app), before);
    }
}

#[test]
fn guarded_screenshot_refuses_changed_document_when_capture_arrives() {
    let (ctx, mut app, _) = two_windows();
    let target = crate::windows::list(&app)[0]["target"].clone();
    let shot = request(&mut app, "ui.screenshot", json!({"window": 0, "target": target, "path": "/tmp/figcraft-stale-target-not-written.png"}));
    ctx.run_ui(RawInput::default(), |ui| crate::control::drain(&mut app, ui.ctx())).textures_delta.clear();
    assert!(shot.try_recv().is_err());
    app.session.execute("file.new", &json!({"name": "New tab before capture"})).unwrap();
    let input = RawInput {
        events: vec![egui::Event::Screenshot {
            viewport_id: ViewportId::ROOT,
            user_data: egui::UserData::default(),
            image: std::sync::Arc::new(egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE])),
        }],
        ..Default::default()
    };
    ctx.run_ui(input, |ui| crate::control::drain(&mut app, ui.ctx())).textures_delta.clear();
    assert!(shot.try_recv().unwrap().result.unwrap_err().contains("target changed"));
}

/// A saved synthetic file shown in hidden window `w`, changed by another writer afterwards.
fn externally_changed(app: &mut FigApp, ctx: &egui::Context, w: u64, name: &str) -> (std::path::PathBuf, Session) {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("figcraft-{name}-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("original.figcraft");
    let saved = request(app, "engine.execute", json!({"window": w, "command": "file.save", "params": {"path": path}}));
    logic(ctx, app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    saved.try_recv().unwrap().result.unwrap();
    let mut external = Session::empty();
    external.execute("file.open", &json!({"path": path})).unwrap();
    external.execute("file.rename", &json!({"name": format!("External {name}")})).unwrap();
    external.execute("file.save", &json!({})).unwrap();
    (path, external)
}

#[test]
fn explicit_refresh_in_a_hidden_window_keeps_focus_and_conflicts() {
    let (ctx, mut app, w) = two_windows();
    let hidden = [ViewportId::ROOT, crate::windows::viewport(w)];
    let (path, mut external) = externally_changed(&mut app, &ctx, w, "hidden");
    let index = app.session.docs.iter().position(|d| d.uid == app.windows.list[0].docs[0]).unwrap();
    let focused = app.windows.focused;
    // The app does not watch files: logic alone leaves the external change unseen.
    logic(&ctx, &mut app, &hidden);
    assert_eq!(app.session.docs[index].doc.name, "A");
    let target = figcraft_engine::target::of(Some(&app.session.docs[index]));
    let refreshed = request(&mut app, "engine.execute", json!({"window": w, "command": "file.refresh", "params": {"target": target}}));
    logic(&ctx, &mut app, &hidden);
    let result = refreshed.try_recv().unwrap().result.unwrap();
    assert_eq!(result["state"], "refreshed");
    assert_eq!(result["target"]["window"], w);
    assert_eq!(app.session.docs[index].doc.name, "External hidden");
    assert_eq!(app.session.ds().unwrap().doc.name, "B", "the main window keeps its document");
    assert_eq!(app.windows.focused, focused);
    let edit = request(&mut app, "engine.execute", json!({"window": w, "command": "file.rename", "params": {"name": "Unsaved local"}}));
    logic(&ctx, &mut app, &hidden);
    edit.try_recv().unwrap().result.unwrap();
    external.execute("file.rename", &json!({"name": "Second external"})).unwrap();
    external.execute("file.save", &json!({})).unwrap();
    let disk = std::fs::read(&path).unwrap();
    let target = figcraft_engine::target::of(Some(&app.session.docs[index]));
    let conflict = request(&mut app, "engine.execute", json!({"window": w, "command": "file.refresh", "params": {"target": target}}));
    logic(&ctx, &mut app, &hidden);
    assert_eq!(conflict.try_recv().unwrap().result.unwrap()["state"], "conflict");
    assert_eq!(app.session.docs[index].doc.name, "Unsaved local");
    assert_eq!(std::fs::read(&path).unwrap(), disk, "a conflict never rewrites the external file");
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn pending_file_drops_defer_explicit_refresh_in_every_window() {
    let (ctx, mut app, w) = two_windows();
    let hidden = [ViewportId::ROOT, crate::windows::viewport(w)];
    let (path, _external) = externally_changed(&mut app, &ctx, w, "drop");
    let disk = std::fs::read(&path).unwrap();
    let index = app.session.docs.iter().position(|d| d.uid == app.windows.list[0].docs[0]).unwrap();
    let focused = app.windows.focused;
    let refresh = |app: &mut FigApp| {
        let target = figcraft_engine::target::of(Some(&app.session.docs[index]));
        let reply = request(app, "engine.execute", json!({"window": w, "command": "file.refresh", "params": {"target": target}}));
        logic(&ctx, app, &hidden);
        reply.try_recv().unwrap().result
    };
    for queued_window in [0, w] {
        crate::windows::as_window(&mut app, queued_window, |app| {
            let ds = app.session.ds().unwrap();
            app.ui.pending_opens.push(crate::pending_open::PendingOpen {
                label: "Pending synthetic import".into(),
                params: json!({}),
                command: "file.import",
                target: Some(crate::pending_open::ImportTarget { uid: ds.uid, page: ds.page, name: ds.doc.name.clone() }),
                read: None,
            });
        });
        assert!(refresh(&mut app).unwrap_err().contains("UI editing"), "pending drop in window {queued_window} blocks replacement");
        assert_eq!(app.session.docs[index].doc.name, "A");
        assert_eq!(std::fs::read(&path).unwrap(), disk);
        crate::windows::as_window(&mut app, queued_window, |app| app.ui.pending_opens.clear());
    }
    assert_eq!(refresh(&mut app).unwrap()["state"], "refreshed");
    assert_eq!(app.session.docs[index].doc.name, "External drop");
    assert_eq!(app.session.ds().unwrap().doc.name, "B");
    assert_eq!(app.windows.focused, focused);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn hidden_windows_answer_ui_inspection_without_a_frame() {
    let (ctx, mut app, w) = two_windows();
    let other = crate::windows::viewport(w);
    for target in [0, w] {
        let ui = request(&mut app, "ui.inspect", json!({"window": target}));
        logic(&ctx, &mut app, &[ViewportId::ROOT, other]);
        let state = ui.try_recv().expect("ui.inspect answers without a frame").result.unwrap();
        assert_eq!(state["window"], json!(target), "inspect the requested window");
        assert_eq!(app.windows.current, 0);
        assert_eq!(app.session.ds().unwrap().doc.name, "B", "restore the main window's document");
        assert!(app.control_queue.is_empty());
    }
}

#[test]
fn closing_a_window_in_the_presenter_shows_the_unsaved_changes_dialog() {
    let mut session = Session::new();
    let frame_id = session.execute("node.create", &json!({"type": "frame", "width": 402, "height": 874})).unwrap()["id"].clone();
    let ctx = egui::Context::default();
    let mut app = FigApp::new(session, Box::new(NoServices));
    frame(&ctx, &mut app, false);
    app.session.execute("proto.present", &json!({"start": frame_id})).unwrap();
    app.handle_ui_requests();
    frame(&ctx, &mut app, false);
    assert!(app.ui.present.is_some());

    frame(&ctx, &mut app, true);
    assert!(app.ui.confirm_close);
    assert!(ctx.memory(|memory| memory.area_rect(egui::Id::new(Some(crate::i18n::tr("Unsaved changes")))).is_some()), "dialog must be on screen");
    frame(&ctx, &mut app, false);
    assert!(app.ui.present.is_some(), "asking to close does not dismiss the preview");
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
fn another_hidden_window_answers_requests_that_need_no_frame_in_its_own_file() {
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
    let rect = |size: u32| json!({"window": w, "command": "shape.rectangle", "params": {"x": 0, "y": 0, "width": size, "height": size}});
    let dirty = |app: &FigApp, name: &str| app.session.docs.iter().find(|d| d.doc.name == name).unwrap().is_dirty();

    // The main window is on screen, the other one covered: the request to it is answered in its
    // file (A), and the main window keeps showing its own (B).
    let edit = request(&mut app, "engine.execute", rect(10));
    logic(&ctx, &mut app, &[other]);
    assert!(edit.try_recv().expect("answered without the other window's frame").result.is_ok());
    assert!(dirty(&app, "A") && !dirty(&app, "B"), "in the other window's file");
    assert_eq!(app.session.ds().unwrap().doc.name, "B");
    assert_eq!(app.windows.current, 0);

    // Both hidden: still answered, and inspection sees the other window's file.
    let inspect = request(&mut app, "document.inspect", json!({"window": w, "depth": 0}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, other]);
    assert_eq!(inspect.try_recv().expect("answered").result.unwrap()["name"], json!("A"));
    assert_eq!(app.session.ds().unwrap().doc.name, "B");

    // What needs a frame, and an unknown window, are errors at once: nothing waits for a window
    // that may stay hidden.
    let shot = request(&mut app, "ui.screenshot", json!({"window": w, "path": "unused.png"}));
    let unknown = request(&mut app, "window.list", json!({"window": 99}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, other]);
    assert!(shot.try_recv().expect("answered").result.unwrap_err().contains(&format!("window {w} is minimised or hidden")));
    assert!(unknown.try_recv().expect("answered").result.unwrap_err().contains("no window 99"));
    assert!(app.control_queue.is_empty());
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
    let ui = request(&mut app, "ui.inspect", json!({"window": w}));
    logic(&ctx, &mut app, &[ViewportId::ROOT]);
    assert!(ui.try_recv().is_err(), "answered in the other window's frame");
    assert!(woken.lock().unwrap().contains(&other), "the other window is asked to draw: {:?}", woken.lock().unwrap());
}

#[test]
fn a_window_not_created_yet_answers_while_the_main_window_is_hidden() {
    let (ctx, mut app, w) = two_windows();
    // The main window's frame creates a new window. While it is hidden, no such frame comes.
    let n = crate::control::handle(&mut app, &ctx, "window.new", &json!({})).unwrap()["window"].as_u64().unwrap();
    assert_eq!(app.windows.focused, n, "untargeted calls go to the new window");
    let input = |main_hidden: bool| {
        let mut input = RawInput::default();
        input.viewports.insert(ViewportId::ROOT, ViewportInfo { minimized: Some(false), occluded: Some(main_hidden), ..Default::default() });
        input.viewports.insert(crate::windows::viewport(w), ViewportInfo { minimized: Some(false), occluded: Some(true), ..Default::default() });
        input
    };
    let screenshot = json!({"window": n, "path": "/tmp/figcraft-uncreated-window-not-written.png"});
    let listed = request(&mut app, "window.list", json!({}));
    let shot = request(&mut app, "ui.screenshot", screenshot.clone());
    let _ = ctx.run_logic(&input(true), |ctx| crate::control::drain_hidden(&mut app, ctx));
    let windows = listed.try_recv().expect("answered without waiting for the window to exist").result.unwrap();
    assert!(windows.as_array().unwrap().iter().any(|v| v["id"] == json!(n)), "{windows}");
    assert!(shot.try_recv().expect("refused at once").result.unwrap_err().contains("minimised or hidden"));
    assert!(app.control_queue.is_empty());
    // On screen, the main window's next frame creates the window: a capture waits for it.
    let shot = request(&mut app, "ui.screenshot", screenshot);
    let _ = ctx.run_logic(&input(false), |ctx| crate::control::drain_hidden(&mut app, ctx));
    assert!(shot.try_recv().is_err(), "waits for the window's first frame");
    assert_eq!(app.control_queue.len(), 1);
}

#[test]
fn a_delayed_screenshot_is_only_asked_for_when_its_time_comes() {
    // `ui.screenshot {after}` (from upstream PR #593): capture once the UI has settled.
    let (ctx, mut app, _) = two_windows();
    let shot = request(&mut app, "ui.screenshot", json!({"window": 0, "path": "/tmp/figcraft-delayed-not-written.png", "after": 0.5}));
    let run = |app: &mut FigApp, time: f64| {
        ctx.run_ui(RawInput { time: Some(time), ..Default::default() }, |ui| crate::control::drain(app, ui.ctx())).textures_delta.clear();
        crate::control::pending(&ctx).lock().unwrap().iter().map(|p| p.asked).collect::<Vec<_>>()
    };
    assert_eq!(run(&mut app, 10.0), vec![false], "not yet");
    assert_eq!(run(&mut app, 10.3), vec![false], "still settling");
    assert_eq!(run(&mut app, 10.6), vec![true], "asked once half a second has passed");
    assert!(shot.try_recv().is_err(), "the capture is still on its way");
}
