//! Guarded headless calls use the same engine identity contract, without app windows.
use crate::{Backend, Headless};
use serde_json::{Value, json};

#[test]
fn file_status_tool_is_read_only_and_returns_the_current_target() {
    let mut h = Headless::new();
    let revision = h.session.ds().unwrap().revision;
    let result = crate::tools::call_tool(&mut h, "file_status", &json!({}));
    assert!(!result.is_error, "{}", result.first_text());
    let status: Value = serde_json::from_str(result.first_text()).unwrap();
    assert_eq!(status["state"], "unsaved");
    assert_eq!(status["target"]["revision"], revision);
    assert_eq!(h.session.ds().unwrap().revision, revision);
    let defs = crate::tools::tool_definitions();
    assert_eq!(defs.iter().find(|d| d["name"] == "file_status").unwrap()["annotations"]["readOnlyHint"], true);
}

#[test]
fn headless_guards_reject_stale_revision_and_other_document_without_mutation() {
    let mut h = Headless::new();
    let mut target = h.call("document.inspect", json!({})).unwrap()["target"].clone();
    h.call("engine.execute", json!({"target": target, "command": "file.rename", "params": {"name": "First"}})).unwrap();
    assert!(
        h.call("engine.execute", json!({"target": target, "command": "file.rename", "params": {"name": "Stale"}})).unwrap_err().contains("revision")
    );
    assert_eq!(h.session.doc().unwrap().name, "First");
    target.as_object_mut().unwrap().remove("revision");
    h.call("engine.execute", json!({"target": target, "command": "file.rename", "params": {"name": "Second"}})).unwrap();
    target["window"] = json!(0);
    assert!(h.call("document.inspect", json!({"target": target})).unwrap_err().contains("window"));
    target.as_object_mut().unwrap().remove("window");
    h.session.execute("file.new", &json!({"name": "New tab"})).unwrap();
    assert!(
        h.call("engine.execute", json!({"target": target, "command": "file.rename", "params": {"name": "Wrong"}}))
            .unwrap_err()
            .contains("target changed")
    );
    assert_eq!(h.session.doc().unwrap().name, "New tab");
}
