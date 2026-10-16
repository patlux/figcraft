//! Saving an old live document must never silently replace a newer external file.

use crate::{Session, cmd::file::FileSnapshot};
use serde_json::json;
use std::path::PathBuf;

struct Fixture {
    dir: PathBuf,
    path: String,
}

#[allow(clippy::unwrap_used)] // Test fixtures only; production code returns errors.
impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("figcraft-conflict-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("design.figcraft").to_string_lossy().into_owned();
        Self { dir, path }
    }

    fn saved(&self) -> Session {
        let mut s = Session::new();
        s.execute("file.save", &json!({"path": self.path})).unwrap();
        s
    }

    fn external(&self) -> Vec<u8> {
        let mut other = Session::empty();
        other.execute("file.open", &json!({"path": self.path})).unwrap();
        other.execute("file.rename", &json!({"name": "External"})).unwrap();
        other.execute("file.save", &json!({})).unwrap();
        std::fs::read(&self.path).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn normal_save_preserves_external_file_and_local_edits() {
    let f = Fixture::new("normal");
    let mut s = f.saved();
    s.execute("file.rename", &json!({"name": "Local"})).unwrap();
    let external = f.external();
    let before = s.ds().unwrap().clone();
    let result = s.execute("file.save", &json!({}));
    assert!(result.is_err(), "an external update must block Cmd+S: {result:?}");
    assert_eq!(std::fs::read(&f.path).unwrap(), external);
    assert_eq!(s.ds().unwrap().doc, before.doc);
    assert_eq!(s.ds().unwrap().revision, before.revision);
    assert!(s.ds().unwrap().is_dirty());
    assert_eq!(s.undo_depth().0, 1);
}

#[test]
fn background_save_refuses_an_external_update_after_encoding() {
    let f = Fixture::new("background");
    let mut s = f.saved();
    s.execute("file.rename", &json!({"name": "Local"})).unwrap();
    let snapshot = FileSnapshot::of(s.ds().unwrap());
    let bytes = snapshot.encode().unwrap();
    let external = f.external();
    assert!(snapshot.save(&mut s, &bytes).is_err(), "autosave must not replace an external update");
    assert_eq!(std::fs::read(&f.path).unwrap(), external);
    assert_eq!(s.ds().unwrap().doc.name, "Local");
    assert!(s.ds().unwrap().is_dirty());
}

#[test]
fn missing_or_corrupt_original_is_not_recreated_or_replaced() {
    for (name, contents) in [("missing", None), ("corrupt", Some(b"broken".as_slice()))] {
        let f = Fixture::new(name);
        let mut s = f.saved();
        if let Some(bytes) = contents {
            std::fs::write(&f.path, bytes).unwrap();
        } else {
            std::fs::remove_file(&f.path).unwrap();
        }
        assert!(s.execute("file.save", &json!({})).is_err());
        assert_eq!(std::fs::read(&f.path).ok().as_deref(), contents);
    }
}

#[test]
fn save_as_does_not_overwrite_an_unobserved_destination() {
    let f = Fixture::new("save-as");
    let _other = f.saved();
    let external = std::fs::read(&f.path).unwrap();
    let mut s = Session::new();
    s.execute("file.rename", &json!({"name": "Mine"})).unwrap();
    assert!(s.execute("file.saveAs", &json!({"path": f.path})).is_err());
    assert_eq!(std::fs::read(&f.path).unwrap(), external);
    assert!(s.ds().unwrap().path.is_none());
    assert!(s.ds().unwrap().is_dirty());
}

#[test]
fn reopening_records_the_external_baseline_and_allows_subsequent_saves() {
    let f = Fixture::new("open");
    let _old = f.saved();
    f.external();
    let mut s = Session::empty();
    s.execute("file.open", &json!({"path": f.path})).unwrap();
    s.execute("file.rename", &json!({"name": "After open"})).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    s.execute("file.rename", &json!({"name": "Second save"})).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    assert!(!s.ds().unwrap().is_dirty());
    assert_eq!(crate::cmd::file::load(&f.path).unwrap().0.name, "Second save");
}
