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
fn poll_precheck_skips_only_unchanged_settled_metadata_of_the_current_baseline() {
    use crate::file_refresh::{Precheck, precheck, remember};
    use std::time::{Duration, SystemTime};
    let f = Fixture::new("precheck");
    let mut s = Session::new();
    s.execute("file.save", &json!({"path": f.path})).unwrap();
    let later = || SystemTime::now() + Duration::from_secs(60);
    // Fresh timestamps can hide a same-size write on coarse file systems: never remember them.
    assert_eq!(precheck(s.ds().unwrap(), SystemTime::now()), Precheck::Read(None));
    let Precheck::Read(Some(stamp)) = precheck(s.ds().unwrap(), later()) else { panic!("settled stamp expected") };
    remember(s.ds_mut().unwrap(), stamp);
    assert_eq!(precheck(s.ds().unwrap(), later()), Precheck::Unchanged);

    // An in-place write of the same size, with the modification time restored, still differs.
    let disk = std::fs::read(&f.path).unwrap();
    let modified = std::fs::metadata(&f.path).unwrap().modified().unwrap();
    let mut altered = disk.clone();
    *altered.last_mut().unwrap() ^= 0xff;
    std::thread::sleep(Duration::from_millis(20));
    let file = std::fs::OpenOptions::new().write(true).open(&f.path).unwrap();
    std::io::Write::write_all(&mut &file, &altered).unwrap();
    file.set_modified(modified).unwrap();
    drop(file);
    assert_eq!(std::fs::metadata(&f.path).unwrap().len(), disk.len() as u64);
    assert!(matches!(precheck(s.ds().unwrap(), later()), Precheck::Read(Some(_))), "status change time differs");
    std::fs::write(&f.path, &disk).unwrap();

    // A replacement by another writer differs, and so does a new baseline after a save.
    let Precheck::Read(Some(stamp)) = precheck(s.ds().unwrap(), later()) else { panic!("settled stamp expected") };
    remember(s.ds_mut().unwrap(), stamp);
    let mut other = Session::empty();
    other.execute("file.open", &json!({"path": f.path})).unwrap();
    other.execute("file.rename", &json!({"name": "Replaced"})).unwrap();
    other.execute("file.save", &json!({})).unwrap();
    assert!(matches!(precheck(s.ds().unwrap(), later()), Precheck::Read(Some(_))), "new inode");
    let reopened = Fixture::new("precheck-save");
    s.execute("file.saveAs", &json!({"path": reopened.path})).unwrap();
    let Precheck::Read(Some(stamp)) = precheck(s.ds().unwrap(), later()) else { panic!("settled stamp expected") };
    remember(s.ds_mut().unwrap(), stamp);
    s.execute("file.rename", &json!({"name": "Saved again"})).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    assert!(matches!(precheck(s.ds().unwrap(), later()), Precheck::Read(_)), "another baseline");
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
