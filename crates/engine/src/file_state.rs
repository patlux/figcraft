//! Explicit disk observations. The saved flag alone never proves live/disk agreement.

use crate::{DocState, EngineError, Result, Session, spec};
use figcraft_format::file_sync::Baseline;
use serde_json::{Value, json};
use std::path::Path;

pub fn specs() -> Vec<crate::cmd::CommandSpec> {
    vec![spec!(
        "file.status",
        "Inspect file synchronization",
        "{} → {target, state, dirty, baseline, disk?, liveDesignSha256?, diskDesignSha256?, designMatchesDisk, savedAndCurrent, error?}. Reads exact disk bytes now, without refreshing or saving. Design hashes exclude the current page, thumbnail and history. savedAndCurrent also requires a clean live document and the saved-byte baseline. An observation, not a lock or future guarantee.",
        false,
        |s, _| status(s)
    )]
}

pub fn baseline_json(b: Option<&Baseline>) -> Value {
    b.map_or(Value::Null, |b| json!({"path": b.path, "sha256": b.sha256, "bytes": b.bytes}))
}

/// A validated native file. Never silently adopt repaired or incomplete content.
pub(crate) fn native(bytes: &[u8]) -> Result<figcraft_format::Loaded> {
    let loaded = figcraft_format::load_bytes_with_versions(bytes).map_err(|e| EngineError::Other(e.to_string()))?;
    if !loaded.issues.is_empty() {
        return Err(EngineError::Other(format!("native file requires repairs: {}", loaded.issues.join("; "))));
    }
    Ok(loaded)
}

/// Hash a native design, including image bytes and published libraries, without history or view.
/// Native encoding has deterministic member order and timestamps. No thumbnail is included.
pub(crate) fn design_hash(doc: &figcraft_doc::Document) -> Result<String> {
    let mut doc = doc.clone();
    doc.current_page = None;
    let bytes = figcraft_format::save_bytes(&doc, None).map_err(|e| EngineError::Other(e.to_string()))?;
    if bytes.len() as u64 > figcraft_format::file_sync::MAX_FILE_BYTES {
        return Err(EngineError::Other("design fingerprint exceeds 512 MiB".into()));
    }
    Ok(figcraft_format::image_hash(&bytes))
}

fn initial(ds: &DocState) -> Value {
    json!({
        "target": crate::target::of(Some(ds)), "path": ds.path,
        "state": "untracked", "dirty": ds.is_dirty(),
        "baseline": baseline_json(ds.file_baseline.as_ref()),
        "designMatchesDisk": false, "savedAndCurrent": false,
    })
}

/// Called directly by the read-only command, before command preprocessing changes previews/revisions.
pub fn status(s: &Session) -> Result<Value> {
    let ds = s.ds()?;
    let mut out = initial(ds);
    if ds.branch_of.is_some() {
        out["state"] = json!("branch");
        out["error"] = json!("inspect the main file tab for disk agreement");
        return Ok(out);
    }
    let Some(path) = &ds.path else {
        out["state"] = json!("unsaved");
        return Ok(out);
    };
    let (bytes, disk) = match figcraft_format::file_sync::read(Path::new(path)) {
        Ok(r) => r,
        Err(e) => {
            out["state"] = json!(if e.kind() == std::io::ErrorKind::NotFound { "missing" } else { "unreadable" });
            out["error"] = json!(e.to_string());
            return Ok(out);
        }
    };
    out["disk"] = baseline_json(Some(&disk));
    if ds.file_baseline.as_ref().is_some_and(|b| b.path != disk.path) {
        out["state"] = json!("pathChanged");
        out["error"] = json!("the file path no longer resolves to the opened destination");
        return Ok(out);
    }
    let loaded = match native(&bytes) {
        Ok(loaded) => loaded,
        Err(e) => {
            out["state"] = json!("invalid");
            out["error"] = json!(e.to_string());
            return Ok(out);
        }
    };
    let live_hash = design_hash(ds.document_itself())?;
    let disk_hash = design_hash(&loaded.doc)?;
    let current = ds.file_baseline.as_ref() == Some(&disk);
    let matches = live_hash == disk_hash;
    out["state"] = json!(if current { "current" } else { "changed" });
    out["liveDesignSha256"] = json!(live_hash);
    out["diskDesignSha256"] = json!(disk_hash);
    out["designMatchesDisk"] = json!(matches);
    out["savedAndCurrent"] = json!(current && matches && !ds.is_dirty());
    Ok(out)
}
