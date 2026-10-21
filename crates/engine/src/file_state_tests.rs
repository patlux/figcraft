//! File status compares the live design with bytes observed now, not just the saved flag.
use crate::Session;
use serde_json::json;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("figcraft-status-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> String {
        self.0.join("design.figcraft").to_string_lossy().into_owned()
    }
    fn saved(&self) -> Session {
        let mut s = Session::new();
        s.execute("shape.rectangle", &json!({"width": 40, "height": 20})).unwrap();
        s.execute("file.save", &json!({"path": self.path()})).unwrap();
        s
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn status_distinguishes_saved_from_disk_agreement_without_editing() {
    let f = Fixture::new("agreement");
    let mut s = f.saved();
    let revision = s.ds().unwrap().revision;
    let status = s.execute("file.status", &json!({})).unwrap();
    assert_eq!(status["state"], "current");
    assert_eq!(status["designMatchesDisk"], true);
    assert_eq!(status["savedAndCurrent"], true);
    assert_eq!(status["baseline"]["sha256"], status["disk"]["sha256"]);
    assert_eq!(s.ds().unwrap().revision, revision, "inspection must not invalidate its own target");
    let mut other = Session::empty();
    other.execute("file.open", &json!({"path": f.path()})).unwrap();
    other.execute("file.rename", &json!({"name": "External"})).unwrap();
    other.execute("file.save", &json!({})).unwrap();
    let status = s.execute("file.status", &json!({})).unwrap();
    assert_eq!(status["state"], "changed");
    assert_eq!(status["dirty"], false);
    assert_eq!(status["designMatchesDisk"], false);
    assert_eq!(status["savedAndCurrent"], false);
    assert_ne!(status["baseline"]["sha256"], status["disk"]["sha256"]);
    assert_eq!(s.ds().unwrap().revision, revision);
}

#[test]
fn status_reports_missing_corrupt_and_unsaved_without_changing_either_side() {
    let f = Fixture::new("errors");
    let mut s = f.saved();
    s.execute("file.rename", &json!({"name": "Local"})).unwrap();
    let status = s.execute("file.status", &json!({})).unwrap();
    assert_eq!(status["state"], "current");
    assert_eq!(status["dirty"], true);
    assert_eq!(status["designMatchesDisk"], false);
    std::fs::write(f.path(), b"corrupt").unwrap();
    let status = s.execute("file.status", &json!({})).unwrap();
    assert_eq!(status["state"], "invalid");
    assert_eq!(status["savedAndCurrent"], false);
    assert_eq!(std::fs::read(f.path()).unwrap(), b"corrupt");
    std::fs::remove_file(f.path()).unwrap();
    assert_eq!(s.execute("file.status", &json!({})).unwrap()["state"], "missing");
    assert_eq!(s.ds().unwrap().doc.name, "Local");
    assert_eq!(s.undo_depth().0, 2);
}

#[test]
fn design_fingerprint_includes_image_bytes_but_not_the_view_page() {
    use std::sync::Arc;
    let mut doc = figcraft_doc::Document::new("Synthetic images");
    doc.images.insert("synthetic".into(), figcraft_doc::ImageData { bytes: Arc::new(vec![1, 2, 3]), width: 1, height: 1 });
    let first = crate::file_state::design_hash(&doc).unwrap();
    doc.current_page = None;
    assert_eq!(crate::file_state::design_hash(&doc).unwrap(), first);
    doc.images.get_mut("synthetic").unwrap().bytes = Arc::new(vec![4, 5, 6]);
    assert_ne!(crate::file_state::design_hash(&doc).unwrap(), first);
}

#[test]
fn status_keeps_history_only_edits_unsaved_and_preserves_previews() {
    let f = Fixture::new("history");
    let mut s = f.saved();
    let version = s.execute("file.saveVersion", &json!({"name": "Retained"})).unwrap();
    let status = s.execute("file.status", &json!({})).unwrap();
    assert_eq!(status["designMatchesDisk"], true);
    assert_eq!(status["savedAndCurrent"], false, "design equality does not prove history persistence");
    s.execute("version.preview", &json!({"id": version["id"]})).unwrap();
    let revision = s.ds().unwrap().revision;
    s.execute("file.status", &json!({})).unwrap();
    assert!(s.ds().unwrap().preview.is_some());
    assert_eq!(s.ds().unwrap().revision, revision);
}

#[test]
fn a_theme_repaint_of_default_page_colours_keeps_the_file_current() {
    // #659 repaints pages still on their theme-given colour when the interface theme changes,
    // without an edit. The file on disk keeps its colour, so status must not report a change.
    let f = Fixture::new("theme");
    let mut s = f.saved();
    s.set_ui_dark(false);
    s.set_ui_dark(true);
    let page = s.ds().unwrap().page;
    let background = |s: &Session| match s.ds().unwrap().doc.node(page).unwrap().kind {
        figcraft_doc::NodeKind::Page { background, .. } => background,
        _ => panic!("not a page"),
    };
    assert_eq!(background(&s), figcraft_color::Color::DARK_PAGE, "the repaint happened");
    let status = s.execute("file.status", &json!({})).unwrap();
    assert_eq!(status["dirty"], false);
    assert_eq!(status["designMatchesDisk"], true, "a theme-given colour is not a design change");
    assert_eq!(status["savedAndCurrent"], true);
    // A chosen colour on disk is still a difference.
    let mut other = Session::empty();
    other.execute("file.open", &json!({"path": f.path()})).unwrap();
    other.execute("page.setBackground", &json!({"color": "#336699"})).unwrap();
    other.execute("file.save", &json!({})).unwrap();
    let status = s.execute("file.status", &json!({})).unwrap();
    assert_eq!(status["designMatchesDisk"], false);
    assert_eq!(status["savedAndCurrent"], false);
}

#[test]
fn an_untitled_file_opened_under_its_file_name_is_current() {
    // Upstream 0.8.0 (#935) names a document the app loads from a file titled "Untitled" after the
    // file. The file on disk keeps its title until the next save, so status must not see a change.
    let f = Fixture::new("untitled");
    let mut doc = figcraft_doc::Document::default();
    doc.name = "Untitled".into();
    std::fs::write(f.path(), figcraft_format::save_bytes(&doc, None).unwrap()).unwrap();
    let mut s = Session::empty();
    s.execute("file.open", &json!({"path": f.path()})).unwrap();
    assert_eq!(s.doc().unwrap().name, "design", "the title follows the file name");
    let status = s.execute("file.status", &json!({})).unwrap();
    assert_eq!(status["dirty"], false);
    assert_eq!(status["designMatchesDisk"], true, "a title the app derived is not a design change");
    assert_eq!(status["savedAndCurrent"], true);
    // A title another program wrote into the file is still a difference.
    let mut other = Session::empty();
    other.execute("file.open", &json!({"path": f.path()})).unwrap();
    other.execute("file.rename", &json!({"name": "Renamed elsewhere"})).unwrap();
    other.execute("file.save", &json!({})).unwrap();
    let status = s.execute("file.status", &json!({})).unwrap();
    assert_eq!(status["designMatchesDisk"], false);
}
