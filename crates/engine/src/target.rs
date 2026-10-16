//! A control target is an app run plus a document, not a reusable window or tab index.

use crate::{DocState, EngineError, Result, Session};
use serde_json::{Value, json};
use std::sync::OnceLock;

/// Process-lifetime identity. This is routing metadata, not authentication or a secret.
pub fn instance() -> &'static str {
    static INSTANCE: OnceLock<String> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let started = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        let process = std::process::id();
        #[cfg(target_arch = "wasm32")]
        let process = 0;
        format!("{process}-{started}")
    })
}

pub fn of(ds: Option<&DocState>) -> Value {
    json!({
        "instance": instance(),
        "document": ds.map(|d| d.uid),
        "path": ds.and_then(|d| d.path.as_ref()),
        "revision": ds.map(|d| d.revision),
    })
}

/// Validate on the session thread immediately before dispatch, not only before queuing.
/// Revision is optional: identity-only pins survive this agent's own edits.
pub fn validate(s: &Session, expected: &Value) -> Result<()> {
    let actual = of(s.ds().ok());
    if !expected.is_object() {
        return Err(EngineError::Other("target must be an object from document.inspect".into()));
    }
    for key in ["instance", "document", "path"] {
        if expected.get(key) != actual.get(key) {
            return Err(EngineError::Other(format!("target changed ({key}); inspect and explicitly select the intended document before retrying")));
        }
    }
    if expected.get("revision").is_some_and(|v| Some(v) != actual.get("revision")) {
        return Err(EngineError::Other("target revision changed; inspect before retrying".into()));
    }
    Ok(())
}
