//! External updates refresh a clean noninteractive document, never discard a conflict.
use crate::Session;
use serde_json::json;
use std::path::PathBuf;

struct Fixture {
    dir: PathBuf,
    path: String,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("figcraft-refresh-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self { path: dir.join("original.figcraft").to_string_lossy().into_owned(), dir }
    }
    fn setup(&self) -> Session {
        let mut s = Session::new();
        s.execute("file.rename", &json!({"name": "Before external update"})).unwrap();
        s.execute("file.saveVersion", &json!({"name": "Prior history"})).unwrap();
        s.execute("file.save", &json!({"path": self.path})).unwrap();
        let mut other = Session::empty();
        other.execute("file.open", &json!({"path": self.path})).unwrap();
        other.execute("file.rename", &json!({"name": "External updated"})).unwrap();
        other.execute("file.save", &json!({})).unwrap();
        s
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn refresh_updates_clean_document_without_reopen_and_preserves_previous_history() {
    let f = Fixture::new("clean");
    let mut s = f.setup();
    let uid = s.ds().unwrap().uid;
    s.ds_mut().unwrap().view.zoom = 2.0;
    let disk = std::fs::read(&f.path).unwrap();
    let r = s.execute("file.refresh", &json!({"target": crate::target::of(s.ds().ok())})).unwrap();
    assert_eq!(r["state"], "refreshed");
    assert_eq!(s.ds().unwrap().uid, uid);
    assert_eq!(s.ds().unwrap().doc.name, "External updated");
    assert_eq!(s.ds().unwrap().view.zoom, 2.0);
    assert!(!s.ds().unwrap().is_dirty());
    assert_eq!(std::fs::read(&f.path).unwrap(), disk, "refresh never rewrites the external file");
    let recovered = crate::cmd::file::load_path(r["recovery"].as_str().unwrap(), None).unwrap().0;
    assert_eq!(recovered.doc.name, "Before external update");
    assert_eq!(recovered.versions[0].name, "Prior history");
    assert_eq!(s.execute("file.status", &json!({})).unwrap()["savedAndCurrent"], true);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.ds().unwrap().doc.name, "Before external update");
}

#[test]
fn status_refresh_and_save_stay_inside_automation_roots() {
    use crate::file_access::{Roots, confined};
    let f = Fixture::new("roots");
    let mut s = f.setup();
    let inside = f.dir.join("inside");
    std::fs::create_dir_all(&inside).unwrap();
    let roots = std::sync::Arc::new(Roots::new(&[], &[inside.to_string_lossy().into_owned()]).unwrap());
    let lock = PathBuf::from(format!("{}.figcraft-lock", f.path));
    std::fs::remove_file(&lock).unwrap();
    let disk = std::fs::read(&f.path).unwrap();
    confined(Some(&roots), || {
        let status = s.execute("file.status", &json!({})).unwrap();
        assert_eq!(status["state"], "unreadable", "{status}");
        assert!(status["error"].as_str().unwrap().contains("automation path rejected"), "{status}");
        let refreshed = s.execute("file.refresh", &json!({"target": crate::target::of(s.ds().ok())})).unwrap();
        assert_eq!(refreshed["state"], "unreadable", "{refreshed}");
        assert!(s.execute("file.save", &json!({})).unwrap_err().to_string().contains("automation path rejected"));
    });
    assert_eq!(s.ds().unwrap().doc.name, "Before external update");
    assert_eq!(std::fs::read(&f.path).unwrap(), disk);
    assert!(!lock.exists(), "no writer lock outside the roots");
    let recoveries = std::fs::read_dir(&f.dir).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains("recovery")).count();
    assert_eq!(recoveries, 0, "no recovery copy outside the roots");
}

#[test]
fn refresh_reports_local_corrupt_missing_and_interactive_conflicts() {
    for case in ["dirty", "corrupt", "missing", "preview"] {
        let f = Fixture::new(case);
        let mut s = f.setup();
        match case {
            "dirty" => {
                s.execute("file.rename", &json!({"name": "Local"})).unwrap();
            }
            "corrupt" => std::fs::write(&f.path, b"broken").unwrap(),
            "missing" => std::fs::remove_file(&f.path).unwrap(),
            "preview" => {
                s.execute("version.preview", &json!({"index": 0})).unwrap();
            }
            _ => assert_eq!(case, "known fixture"),
        }
        let before = s.ds().unwrap().clone();
        let disk = std::fs::read(&f.path).ok();
        let r = s.execute("file.refresh", &json!({"target": crate::target::of(s.ds().ok())})).unwrap();
        assert!(matches!(r["state"].as_str(), Some("conflict" | "invalid" | "missing" | "deferred")), "{case}: {r}");
        assert_eq!(s.ds().unwrap().doc, before.doc, "{case}");
        assert_eq!(s.ds().unwrap().preview.is_some(), before.preview.is_some());
        assert_eq!(s.ds().unwrap().revision, before.revision);
        assert_eq!(std::fs::read(&f.path).ok(), disk);
    }
}
