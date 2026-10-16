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
    let target = crate::windows::list(&app)[0]["target"].clone();
    let raw = request(&mut app, "ui.input", json!({"window": 0, "target": target, "events": [{"text": "not deferred"}]}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    assert!(raw.try_recv().unwrap().result.unwrap_err().contains("deferred"));
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

#[test]
fn hidden_window_adopts_candidate_in_place_without_changing_focus_or_other_document() {
    let (ctx, mut app, w) = two_windows();
    let dir = std::env::temp_dir().join(format!("figcraft-hidden-adopt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("original.figcraft");
    let candidate = dir.join("candidate.figcraft");
    let saved = request(&mut app, "engine.execute", json!({"window": w, "command": "file.save", "params": {"path": path}}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    assert!(saved.try_recv().unwrap().result.is_ok());
    let mut generated = Session::new();
    generated.execute("file.rename", &json!({"name": "Generated hidden"})).unwrap();
    generated.execute("file.save", &json!({"path": candidate})).unwrap();
    let status = request(&mut app, "engine.execute", json!({"window": w, "command": "file.status"}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    let status = status.try_recv().unwrap().result.unwrap();
    assert_eq!(status["target"]["window"], w);
    let (_, candidate_baseline) = figcraft_format::file_sync::read(&candidate).unwrap();
    let focused = app.windows.focused;
    let adoption = request(
        &mut app,
        "engine.execute",
        json!({"window": w, "target": status["target"], "command": "file.adopt", "params": {
            "target": status["target"], "candidate": candidate, "candidateSha256": candidate_baseline.sha256,
            "diskSha256": status["disk"]["sha256"], "operation": "hidden-window-test"
        }}),
    );
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    let result = adoption.try_recv().unwrap().result.unwrap();
    assert_eq!(result["target"]["document"], status["target"]["document"]);
    assert_eq!(result["target"]["window"], w);
    assert_eq!(app.windows.focused, focused);
    assert_eq!(app.session.ds().unwrap().doc.name, "B");
    let inspected = request(&mut app, "document.inspect", json!({"window": w}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    assert_eq!(inspected.try_recv().unwrap().result.unwrap()["name"], "Generated hidden");
    assert_eq!(figcraft_engine::cmd::file::load(path.to_str().unwrap()).unwrap().0.name, "Generated hidden");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn external_poll_refreshes_hidden_tab_and_retains_dirty_conflict_without_focus_change() {
    let (ctx, mut app, w) = two_windows();
    let dir = std::env::temp_dir().join(format!("figcraft-hidden-refresh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("original.figcraft");
    let saved = request(&mut app, "engine.execute", json!({"window": w, "command": "file.save", "params": {"path": path}}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    assert!(saved.try_recv().unwrap().result.is_ok());
    let mut external = Session::empty();
    external.execute("file.open", &json!({"path": path})).unwrap();
    external.execute("file.rename", &json!({"name": "External hidden"})).unwrap();
    external.execute("file.save", &json!({})).unwrap();
    let focused = app.windows.focused;
    let index = app.session.docs.iter().position(|d| d.doc.name == "A").unwrap();
    app.external_poll_cursor = index;
    app.next_external_poll = None;
    let mut input = RawInput { time: Some(10.0), ..Default::default() };
    for id in [ViewportId::ROOT, crate::windows::viewport(w)] {
        input.viewports.insert(id, ViewportInfo { occluded: Some(true), ..Default::default() });
    }
    let input_request = request(&mut app, "ui.input", json!({"window": w, "events": [{"text": "pending field input"}]}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    assert!(input_request.try_recv().unwrap().result.is_ok());
    let _ = ctx.run_logic(&input, |ctx| app.external_file_tick(ctx));
    assert_eq!(app.session.docs[index].doc.name, "A", "deferred input prevents automatic replacement");
    assert_eq!(app.session.docs[index].external_file_state.as_ref().unwrap()["state"], "deferred");
    let target = figcraft_engine::target::of(Some(&app.session.docs[index]));
    let guarded = request(&mut app, "engine.execute", json!({"window": w, "command": "file.refresh", "params": {"target": target}}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    assert!(guarded.try_recv().unwrap().result.unwrap_err().contains("UI editing"));
    assert!(!crate::windows::take_input(&mut app, crate::windows::viewport(w)).is_empty());
    app.external_poll_cursor = index;
    app.next_external_poll = Some(std::time::Instant::now());
    let _ = ctx.run_logic(&input, |ctx| app.external_file_tick(ctx));
    assert_eq!(app.session.docs[index].doc.name, "External hidden");
    assert_eq!(app.session.docs[index].external_file_state.as_ref().unwrap()["state"], "refreshed");
    assert_eq!(app.session.ds().unwrap().doc.name, "B");
    assert_eq!(app.windows.focused, focused);
    let edit = request(&mut app, "engine.execute", json!({"window": w, "command": "file.rename", "params": {"name": "Unsaved local"}}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, crate::windows::viewport(w)]);
    assert!(edit.try_recv().unwrap().result.is_ok());
    external.execute("file.rename", &json!({"name": "Second external"})).unwrap();
    external.execute("file.save", &json!({})).unwrap();
    app.external_poll_cursor = index;
    input.time = Some(20.0);
    app.next_external_poll = Some(std::time::Instant::now());
    let _ = ctx.run_logic(&input, |ctx| app.external_file_tick(ctx));
    assert_eq!(app.session.docs[index].doc.name, "Unsaved local");
    assert_eq!(app.session.docs[index].external_file_state.as_ref().unwrap()["state"], "conflict");
    assert_eq!(figcraft_engine::cmd::file::load(path.to_str().unwrap()).unwrap().0.name, "Second external");
    assert_eq!(app.windows.focused, focused);
    std::fs::remove_dir_all(dir).unwrap();
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
fn hidden_windows_refuse_screenshots_without_waiting_for_a_frame() {
    let (ctx, mut app, w) = two_windows();
    let other = crate::windows::viewport(w);
    for target in [0, w] {
        let shot = request(&mut app, "ui.screenshot", json!({"window": target, "path": "/tmp/figcraft-hidden-not-written.png"}));
        logic(&ctx, &mut app, &[ViewportId::ROOT, other]);
        let error = shot.try_recv().expect("screenshot is refused immediately").result.unwrap_err();
        assert!(error.contains("visible"), "{error}");
        assert!(app.control_queue.is_empty());
    }
}

#[test]
fn hidden_windows_buffer_raw_input_for_only_their_own_next_frame() {
    let (ctx, mut app, w) = two_windows();
    let other = crate::windows::viewport(w);
    for target in [0, w] {
        let input = request(&mut app, "ui.input", json!({"window": target, "events": [{"text": "synthetic"}]}));
        logic(&ctx, &mut app, &[ViewportId::ROOT, other]);
        assert!(input.try_recv().expect("raw input is accepted without a frame").result.is_ok());
        let not_target = if target == 0 { other } else { ViewportId::ROOT };
        assert!(crate::windows::take_input(&mut app, not_target).is_empty());
        let events = crate::windows::take_input(&mut app, crate::windows::viewport(target));
        assert!(matches!(events.as_slice(), [egui::Event::Text(text)] if text == "synthetic"));
        assert!(crate::windows::take_input(&mut app, crate::windows::viewport(target)).is_empty());
        assert!(app.session.docs.iter().all(|d| !d.is_dirty()), "input is buffered, not applied yet");
        assert_eq!(app.windows.current, 0);
        assert!(app.control_queue.is_empty());
    }
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

    // UI inspection also answers without a frame; an unknown window is an error at once.
    let ui = request(&mut app, "ui.inspect", json!({"window": w}));
    let unknown = request(&mut app, "window.list", json!({"window": 99}));
    logic(&ctx, &mut app, &[ViewportId::ROOT, other]);
    assert_eq!(ui.try_recv().expect("UI inspection answered").result.unwrap()["window"], json!(w));
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
