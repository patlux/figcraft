//! App-owned replacement of a clean open document from a separate native candidate.
//! No automatic replay. A named version records successful results across reopen/restart.

use crate::{EngineError, Result, Session, bad, file_state, params::req_string, spec};
#[cfg(not(target_arch = "wasm32"))]
use figcraft_format::Version;
use serde_json::{Value, json};

const ADOPT: &str = "file.adopt";
const STATUS: &str = "file.operationStatus";

pub fn specs() -> Vec<crate::cmd::CommandSpec> {
    vec![
        spec!(
            ADOPT,
            "Adopt generated file",
            "{target (including revision from file.status), candidate (separate native .figcraft path), candidateSha256, diskSha256, operation (unique 1-64 ASCII letters/digits/_/-)} → {operation, recovery, target, disk, designSha256}. Requires a clean, noninteractive main file, no live collaboration or open branch tabs. Validates both files under participating-writer locks, retains history and a separate recovery copy, writes atomically, then updates the same document. Never replay an unknown outcome: query file.operationStatus.",
            false,
            adopt
        ),
        spec!(
            STATUS,
            "Inspect file operation",
            "{operation} → {outcome: resultPresent|changedOrUnknown|unknown, savedAndCurrent, receipt?, fileStatus}. A resultPresent observation requires the retained receipt, matching design, saved baseline and current disk bytes. Unknown does not mean safe to retry. No replay.",
            false,
            operation_status
        ),
    ]
}

fn operation(p: &Value, cmd: &str) -> Result<String> {
    let op = req_string(cmd, p, "operation")?;
    if op.is_empty() || op.len() > 64 || !op.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return Err(bad(cmd, "operation must contain 1-64 ASCII letters, digits, underscores or hyphens"));
    }
    Ok(op)
}

fn receipt_id(op: &str) -> String {
    // 64 hex digits plus a prefix exceeds the native version-id limit. Use 56 hex digits.
    let hash = figcraft_format::image_hash(op.as_bytes());
    format!("op-{}", hash.chars().take(56).collect::<String>())
}

fn receipt(ds: &crate::DocState, op: &str) -> Option<Value> {
    let id = receipt_id(op);
    let v = ds.versions.iter().find(|v| v.id == id)?;
    let record: Value = serde_json::from_str(&v.description).ok()?;
    (record["operation"] == op && record["figcraftAdoption"] == 1).then_some(record)
}

pub fn operation_status(s: &mut Session, p: &Value) -> Result<Value> {
    let op = operation(p, STATUS)?;
    let record = receipt(s.ds()?, &op);
    let status = file_state::status(s)?;
    let present = record.as_ref().is_some_and(|r| status["savedAndCurrent"] == true && status["liveDesignSha256"] == r["designSha256"]);
    Ok(json!({"operation": op, "outcome": if present { "resultPresent" } else if record.is_some() { "changedOrUnknown" } else { "unknown" },
        "savedAndCurrent": status["savedAndCurrent"], "receipt": record, "fileStatus": status}))
}

/// Check before command preprocessing: even a rejected adoption must preserve interactive state.
pub(crate) fn ready(s: &Session) -> Result<()> {
    if s.group_depth != 0 {
        return Err(bad(ADOPT, "adoption is not allowed inside a rollback-capable command group"));
    }
    let ds = s.ds()?;
    if ds.is_dirty() {
        return Err(bad(ADOPT, "local changes must be saved or preserved separately before adoption"));
    }
    if ds.interaction.is_some() || ds.text_edit.is_some() || ds.vector_edit.is_some() || ds.preview.is_some() || ds.text_preview.is_some() {
        return Err(bad(ADOPT, "the document is being edited or previewed; no replacement is allowed"));
    }
    if ds.branch_of.is_some() || s.docs.iter().any(|d| d.branch_of.as_ref().is_some_and(|(uid, _)| *uid == ds.uid)) {
        return Err(bad(ADOPT, "close branch tabs before replacing the main file"));
    }
    if s.live_of(ds.uid).doc_uid.is_some() {
        return Err(bad(ADOPT, "leave live collaboration before replacing the file"));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn adopt(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(EngineError::Other("no file system in the browser".into()))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn adopt(s: &mut Session, p: &Value) -> Result<Value> {
    use figcraft_format::file_sync::{Guard, read, target, write_checked};
    use std::{path::Path, sync::Arc};

    let expected = p.get("target").ok_or_else(|| bad(ADOPT, "target from file.status is required"))?;
    if expected.get("revision").and_then(Value::as_u64).is_none() {
        return Err(bad(ADOPT, "target revision from file.status is required"));
    }
    crate::target::validate(s, expected)?;
    ready(s)?;
    let op = operation(p, ADOPT)?;
    let ds = s.ds()?;
    if ds.versions.iter().any(|v| v.id == receipt_id(&op)) {
        return Err(bad(ADOPT, "operation already recorded; query file.operationStatus instead of replaying"));
    }
    let path = ds.path.as_deref().ok_or_else(|| bad(ADOPT, "save the original to a native file first"))?;
    if !Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("figcraft")) {
        return Err(bad(ADOPT, "adoption requires an original .figcraft file"));
    }
    let baseline = ds.file_baseline.as_ref().ok_or_else(|| bad(ADOPT, "original has no file baseline"))?;
    if req_string(ADOPT, p, "diskSha256")? != baseline.sha256 {
        return Err(bad(ADOPT, "expected disk hash differs from the live saved baseline"));
    }
    let candidate = req_string(ADOPT, p, "candidate")?;
    let candidate_path = target(Path::new(&candidate)).map_err(io)?;
    if candidate_path == baseline.path {
        return Err(bad(ADOPT, "candidate must be a separate file, never the open original"));
    }
    let destination = Guard::acquire(Path::new(path)).map_err(io)?;
    destination.verify(Some(baseline)).map_err(io)?;
    let candidate_guard = Guard::acquire(Path::new(&candidate)).map_err(io)?;
    let (candidate_bytes, candidate_baseline) = read(Path::new(&candidate)).map_err(io)?;
    if req_string(ADOPT, p, "candidateSha256")? != candidate_baseline.sha256 {
        return Err(bad(ADOPT, "candidate hash changed; inspect the separate candidate again"));
    }
    let loaded = file_state::native(&candidate_bytes)?;
    if !loaded.branches.is_empty() {
        return Err(bad(ADOPT, "generator candidates with branches need explicit branch reconciliation"));
    }
    if ds.versions.len().saturating_add(loaded.versions.len()).saturating_add(2) > figcraft_format::MAX_VERSIONS {
        return Err(bad(ADOPT, "history has insufficient space; no versions were removed"));
    }
    let mut next = ds.clone();
    let before = ds.doc.clone();
    next.doc = Arc::new(loaded.doc);
    next.page = if next.doc.pages.contains(&ds.page) { ds.page } else { next.doc.first_page().ok_or_else(|| bad(ADOPT, "candidate has no page"))? };
    next.selection.clear();
    next.hover = None;
    next.focus = None;
    next.guide = None;
    next.rotation_origin = None;
    next.last_duplicate = None;
    next.push_undo("Adopt generated file", before.clone(), ds.selection.clone(), ds.page);
    let now = crate::cmd::comment::now_secs();
    let before_id = unique_version_id(&next.versions);
    next.versions.push(Version::new(before_id, "Before generator adoption", &op, now, before));
    for mut version in loaded.versions {
        if next.versions.iter().any(|v| v.id == version.id) || version.id == receipt_id(&op) {
            version.id = unique_version_id(&next.versions);
        }
        next.versions.push(version);
    }
    let recovery = format!("{}.recovery-{op}.figcraft", baseline.path.to_string_lossy());
    let design_hash = file_state::design_hash(&next.doc)?;
    let record = json!({"figcraftAdoption": 1, "operation": op, "candidateSha256": candidate_baseline.sha256,
        "designSha256": design_hash, "recovery": recovery});
    let description = record.to_string();
    if description.chars().count() > figcraft_format::MAX_VERSION_DESCRIPTION {
        return Err(bad(ADOPT, "operation receipt exceeds native history limits"));
    }
    next.versions.push(Version::new(receipt_id(&op), "Generator adoption result", &description, now, next.doc.clone()));
    let bytes = crate::cmd::file::file_bytes(&next)?;
    // Encode every potentially failing part before touching the original. The recovery path must be new.
    let recovery_bytes = crate::cmd::file::file_bytes(ds)?;
    write_checked(Path::new(&recovery), None, &recovery_bytes).map_err(io)?;
    candidate_guard.verify(Some(&candidate_baseline)).map_err(io)?;
    crate::target::validate(s, expected)?;
    let baseline = baseline.clone();
    let slot = s.docs.get_mut(s.active).ok_or(crate::EngineError::NoDocument)?;
    let saved = destination.replace(Some(&baseline), &bytes).map_err(io)?;
    // From here onward only infallible state installation. Generic command normalization must not run.
    next.file_baseline = Some(saved.clone());
    next.mark_saved();
    next.mark_checkpoint();
    next.revision = next.revision.wrapping_add(1);
    *slot = next;
    let target = crate::target::of(Some(slot));
    Ok(json!({"operation": op, "recovery": recovery, "target": target, "disk": file_state::baseline_json(Some(&saved)), "designSha256": design_hash}))
}

#[cfg(not(target_arch = "wasm32"))]
fn io(e: std::io::Error) -> EngineError {
    EngineError::Other(e.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn unique_version_id(versions: &[Version]) -> String {
    crate::cmd::file::unique_id(|id| versions.iter().any(|v| v.id == id))
}
