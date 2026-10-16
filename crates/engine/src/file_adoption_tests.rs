//! Generator candidates are adopted only by the owner of the open file, under writer locks.
use crate::{Session, target};
use serde_json::{Value, json};
use std::path::PathBuf;

struct Fixture {
    dir: PathBuf,
    original: String,
    candidate: String,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("figcraft-adopt-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            original: dir.join("original.figcraft").to_string_lossy().into_owned(),
            candidate: dir.join("candidate.figcraft").to_string_lossy().into_owned(),
            dir,
        }
    }
    fn setup(&self) -> (Session, Value) {
        let mut live = Session::new();
        live.execute("file.rename", &json!({"name": "Before"})).unwrap();
        live.execute("file.saveVersion", &json!({"name": "Original named version"})).unwrap();
        live.execute("file.save", &json!({"path": self.original})).unwrap();
        let mut candidate = Session::new();
        candidate.execute("file.rename", &json!({"name": "Generated"})).unwrap();
        candidate.execute("file.saveVersion", &json!({"name": "Candidate named version"})).unwrap();
        candidate.execute("file.save", &json!({"path": self.candidate})).unwrap();
        let (_, baseline) = figcraft_format::file_sync::read(std::path::Path::new(&self.candidate)).unwrap();
        let params = json!({"target": target::of(live.ds().ok()), "candidate": self.candidate, "candidateSha256": baseline.sha256,
            "diskSha256": live.ds().unwrap().file_baseline.as_ref().unwrap().sha256, "operation": "synthetic-operation"});
        (live, params)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn adoption_updates_the_existing_document_and_retains_history_and_recovery() {
    let f = Fixture::new("success");
    let (mut s, p) = f.setup();
    s.ds_mut().unwrap().view.zoom = 2.75;
    let uid = s.ds().unwrap().uid;
    let view = s.ds().unwrap().view.clone();
    let undo = s.undo_depth().0;
    let r = s.execute("file.adopt", &p).unwrap();
    assert_eq!(s.docs.len(), 1);
    assert_eq!(s.ds().unwrap().uid, uid);
    assert_eq!(s.ds().unwrap().view, view);
    assert_eq!(s.ds().unwrap().doc.name, "Generated");
    assert!(!s.ds().unwrap().is_dirty());
    assert_eq!(s.undo_depth().0, undo + 1);
    let versions = s.execute("version.list", &json!({})).unwrap();
    assert!(versions.as_array().unwrap().iter().any(|v| v["name"] == "Original named version"));
    assert!(versions.as_array().unwrap().iter().any(|v| v["name"] == "Candidate named version"));
    let recovery = r["recovery"].as_str().unwrap();
    let recovered = crate::cmd::file::load_path(recovery, None).unwrap().0;
    assert_eq!(recovered.doc.name, "Before");
    assert_eq!(recovered.versions[0].name, "Original named version");
    assert_eq!(s.execute("file.status", &json!({})).unwrap()["savedAndCurrent"], true);
    let persisted = crate::cmd::file::load_path(&f.original, None).unwrap().0;
    assert!(persisted.versions.iter().any(|v| v.name == "Original named version"));
    assert!(persisted.versions.iter().any(|v| v.name == "Candidate named version"));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.ds().unwrap().doc.name, "Before");
    assert!(s.ds().unwrap().is_dirty());
    s.execute("file.save", &json!({})).unwrap();
    assert_eq!(crate::cmd::file::load(&f.original).unwrap().0.name, "Before");
    assert_eq!(s.execute("file.operationStatus", &json!({"operation": p["operation"]})).unwrap()["outcome"], "changedOrUnknown");
}

#[test]
fn lost_adoption_reply_is_resolved_by_observation_not_replay() {
    let f = Fixture::new("lost-reply");
    let (mut s, p) = f.setup();
    let _lost = s.execute("file.adopt", &p).unwrap();
    let status = s.execute("file.operationStatus", &json!({"operation": p["operation"]})).unwrap();
    assert_eq!(status["outcome"], "resultPresent");
    assert_eq!(status["savedAndCurrent"], true);
    let bytes = std::fs::read(&f.original).unwrap();
    let mut retry = p.clone();
    retry["target"] = target::of(s.ds().ok());
    retry["diskSha256"] = json!(s.ds().unwrap().file_baseline.as_ref().unwrap().sha256);
    assert!(s.execute("file.adopt", &retry).is_err(), "same operation is never replayed");
    assert_eq!(std::fs::read(&f.original).unwrap(), bytes);
    let mut reopened = Session::empty();
    reopened.execute("file.open", &json!({"path": f.original})).unwrap();
    assert_eq!(reopened.execute("file.operationStatus", &json!({"operation": p["operation"]})).unwrap()["outcome"], "resultPresent");
}

#[test]
fn adoption_cannot_escape_a_rolled_back_batch() {
    let f = Fixture::new("batch");
    let (mut s, p) = f.setup();
    let bytes = std::fs::read(&f.original).unwrap();
    let err = s.batch("Forbidden disk transaction", &[("file.adopt".into(), p), ("not.a.command".into(), json!({}))]).unwrap_err();
    assert!(err.to_string().contains("command group"), "{err}");
    assert_eq!(std::fs::read(&f.original).unwrap(), bytes);
    assert_eq!(s.ds().unwrap().doc.name, "Before");
    assert_eq!(s.group_depth, 0);
}

#[test]
fn adoption_refuses_local_external_candidate_and_target_changes() {
    for case in ["local", "external", "candidate", "target", "revision", "busy", "invalid", "recovery-write", "preview", "full-history"] {
        let f = Fixture::new(case);
        let (mut s, mut p) = f.setup();
        let mut lock = None;
        match case {
            "local" => {
                s.execute("file.rename", &json!({"name": "Local"})).unwrap();
                p["target"] = target::of(s.ds().ok());
            }
            "external" => {
                let mut ext = Session::empty();
                ext.execute("file.open", &json!({"path": f.original})).unwrap();
                ext.execute("file.rename", &json!({"name": "External"})).unwrap();
                ext.execute("file.save", &json!({})).unwrap();
            }
            "candidate" => {
                std::fs::write(&f.candidate, b"changed candidate").unwrap();
            }
            "target" => {
                p["target"]["document"] = json!(99999);
            }
            "revision" => {
                s.execute("select.all", &json!({})).unwrap();
            }
            "busy" => {
                lock = Some(figcraft_format::file_sync::Guard::acquire(std::path::Path::new(&f.original)).unwrap());
            }
            "invalid" => {
                std::fs::write(&f.candidate, b"invalid").unwrap();
                p["candidateSha256"] = json!(figcraft_format::image_hash(b"invalid"));
            }
            "recovery-write" => {
                std::fs::create_dir(format!("{}.recovery-synthetic-operation.figcraft", f.original)).unwrap();
            }
            "preview" => {
                s.execute("version.preview", &json!({"index": 0})).unwrap();
                p["target"] = target::of(s.ds().ok());
            }
            "full-history" => {
                let ds = s.ds_mut().unwrap();
                for n in ds.versions.len()..figcraft_format::MAX_VERSIONS {
                    ds.versions.push(figcraft_format::Version::new(format!("full-{n}"), "Named version", "", 0, ds.doc.clone()));
                }
                s.execute("file.save", &json!({})).unwrap();
                p["target"] = target::of(s.ds().ok());
                p["diskSha256"] = json!(s.ds().unwrap().file_baseline.as_ref().unwrap().sha256);
            }
            _ => assert_eq!(case, "unknown fixture case"),
        }
        let before = s.ds().unwrap().clone();
        let disk = std::fs::read(&f.original).unwrap();
        let error = s.execute("file.adopt", &p).unwrap_err();
        assert!(!matches!(error, crate::EngineError::UnknownCommand(_)), "a refusal must exercise adoption, not an absent command");
        assert_eq!(s.ds().unwrap().doc, before.doc, "{case}");
        assert_eq!(s.ds().unwrap().revision, before.revision, "{case}");
        assert_eq!(s.ds().unwrap().versions.len(), before.versions.len(), "{case}");
        assert_eq!(s.ds().unwrap().preview.is_some(), before.preview.is_some(), "{case}");
        assert_eq!(std::fs::read(&f.original).unwrap(), disk, "{case}");
        drop(lock);
    }
}
