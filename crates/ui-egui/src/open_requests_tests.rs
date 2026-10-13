//! Files the system hands over (macOS: a Finder double-click, Open With, a drop on the Dock icon)
//! open like File › Open in the window that draws next; the untouched empty file the app started
//! with gives way to them.

use std::path::PathBuf;

use figcraft_engine::Session;
use serde_json::json;

use crate::{FigApp, NoServices};

fn frame(ctx: &egui::Context, app: &mut FigApp) {
    ctx.run_ui(egui::RawInput::default(), |ui| app.ui(ui)).textures_delta.clear();
}

/// A saved `.figcraft` file named `name`, in a temporary folder of its own.
fn saved(name: &str) -> String {
    let dir: PathBuf = std::env::temp_dir().join(format!("figcraft-open-requests-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.figcraft")).to_string_lossy().into_owned();
    let mut s = Session::empty();
    s.execute("file.new", &json!({"name": name})).unwrap();
    s.execute("file.saveAs", &json!({"path": path})).unwrap();
    path
}

/// The app as the desktop app starts it with nothing to open: one empty file.
fn started() -> (egui::Context, FigApp) {
    let mut s = Session::empty();
    s.execute("file.new", &json!({})).unwrap();
    let uid = s.ds().unwrap().uid;
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut app = FigApp::new(s, Box::new(NoServices));
    app.startup_file = Some(uid);
    // The first frame only installs the fonts.
    frame(&ctx, &mut app);
    (ctx, app)
}

fn names(app: &FigApp) -> Vec<String> {
    app.session.docs.iter().map(|d| d.doc.name.clone()).collect()
}

#[test]
fn handed_over_files_open_and_replace_the_untouched_startup_file() {
    let (ctx, mut app) = started();
    app.ui.home = true;
    app.request_open(&ctx, [saved("Plan"), saved("Sketch")]);
    assert_eq!(app.open_requests.len(), 2, "they wait for a window to draw");
    frame(&ctx, &mut app);
    assert_eq!(names(&app), ["Plan", "Sketch"]);
    assert_eq!(app.session.ds().unwrap().doc.name, "Sketch", "the last one handed over shows");
    assert!(app.open_requests.is_empty());
    assert!(!app.ui.home, "Home closes, as after File › Open");
}

#[test]
fn an_open_file_comes_to_the_front_and_a_failure_shows() {
    let (ctx, mut app) = started();
    let again = saved("Again");
    app.request_open(&ctx, [again.clone()]);
    frame(&ctx, &mut app);
    app.session.execute("file.new", &json!({"name": "Other"})).unwrap();
    let missing = again.replace("Again.figcraft", "missing.figcraft");
    app.request_open(&ctx, [missing, again]);
    frame(&ctx, &mut app);
    assert_eq!(names(&app), ["Again", "Other"], "a file open already isn't opened twice");
    assert_eq!(app.session.ds().unwrap().doc.name, "Again", "it comes to the front");
    assert!(app.ui.last_error.is_some(), "the missing file's error shows");
}

#[test]
fn a_startup_file_with_changes_stays() {
    let (ctx, mut app) = started();
    app.session.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
    app.request_open(&ctx, [saved("Kept")]);
    frame(&ctx, &mut app);
    assert_eq!(names(&app).len(), 2, "the edited file stays: {:?}", names(&app));
    assert_eq!(app.session.ds().unwrap().doc.name, "Kept");
}

#[test]
fn the_startup_file_stands_in_until_a_file_opens() {
    let (ctx, mut app) = started();
    app.request_open(&ctx, [std::env::temp_dir().join("figcraft-no-such-file.figcraft").to_string_lossy().into_owned()]);
    frame(&ctx, &mut app);
    assert_eq!(names(&app).len(), 1, "a failed open closes nothing");
    app.request_open(&ctx, [saved("Later")]);
    frame(&ctx, &mut app);
    assert_eq!(names(&app), ["Later"], "the next file that opens replaces it");
}
