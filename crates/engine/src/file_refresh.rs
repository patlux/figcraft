//! External native-file updates: refresh only clean, noninteractive documents; retain recovery.

use crate::{EngineError, Result, Session, spec};
use serde_json::{Value, json};

pub fn specs() -> Vec<crate::cmd::CommandSpec> {
    vec![spec!(
        "file.refresh",
        "Refresh external file",
        "{target (from file.status or document.inspect)} → {state: current|refreshed|conflict|deferred|missing|invalid|unreadable|pathChanged|untracked, recovery?, error?, target}. Checks an external native-file update under the writer lock. Only clean, noninteractive main files refresh. Retains a separate complete recovery copy and one undo step, preserves document identity/view, and never rewrites the external file or steals focus.",
        false,
        refresh
    )]
}

#[cfg(target_arch = "wasm32")]
pub fn refresh(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(EngineError::Other("no file system in the browser".into()))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn refresh(s: &mut Session, p: &Value) -> Result<Value> {
    use figcraft_format::file_sync::{Guard, read, write_checked};
    use std::{path::Path, sync::Arc};
    let target = p.get("target").ok_or_else(|| crate::bad("file.refresh", "target is required"))?;
    crate::target::validate(s, target)?;
    let ds = s.ds()?;
    let reply = |state: &str, error: Option<String>| json!({"state": state, "error": error, "target": crate::target::of(Some(ds))});
    if s.group_depth != 0 {
        return Err(crate::bad("file.refresh", "refresh is not allowed inside a rollback-capable command group"));
    }
    let (Some(path), Some(baseline)) = (&ds.path, &ds.file_baseline) else {
        return Ok(reply("untracked", None));
    };
    if ds.branch_of.is_some() || !Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("figcraft")) {
        return Ok(reply("untracked", None));
    }
    let error_state = |e: &std::io::Error| if e.kind() == std::io::ErrorKind::NotFound { "missing" } else { "unreadable" };
    let guard = match Guard::acquire(Path::new(path)) {
        Ok(guard) => guard,
        Err(e) => return Ok(reply(error_state(&e), Some(e.to_string()))),
    };
    let (bytes, observed) = match read(Path::new(path)) {
        Ok(read) => read,
        Err(e) => return Ok(reply(error_state(&e), Some(e.to_string()))),
    };
    if observed.path != baseline.path {
        return Ok(reply("pathChanged", Some("the original path resolves to another destination".into())));
    }
    if observed == *baseline {
        return Ok(reply("current", None));
    }
    let loaded = match crate::file_state::native(&bytes) {
        Ok(loaded) => loaded,
        Err(e) => return Ok(reply("invalid", Some(e.to_string()))),
    };
    if ds.is_dirty() {
        return Ok(reply("conflict", Some("external file changed while local edits are unsaved; both versions are retained".into())));
    }
    if let Err(e) = crate::file_adoption::ready(s) {
        return Ok(reply("deferred", Some(e.to_string())));
    }
    let mut next = ds.clone();
    next.doc = Arc::new(loaded.doc);
    next.versions = loaded.versions;
    next.branches = loaded.branches;
    next.page = if next.doc.pages.contains(&ds.page) {
        ds.page
    } else {
        next.doc.first_page().ok_or_else(|| crate::bad("file.refresh", "file has no page"))?
    };
    next.clean_selection();
    next.hover = None;
    next.focus = None;
    next.guide = None;
    next.rotation_origin = None;
    next.last_duplicate = None;
    next.push_undo("Refresh external file", ds.doc.clone(), ds.selection.clone(), ds.page);
    // A unique create-only recovery is authoritative for the previous version/branch history.
    // Importing old history into the new document would make its clean saved flag dishonest.
    let recovery = format!(
        "{}.recovery-refresh-{}-{}-{}-{}.figcraft",
        baseline.path.to_string_lossy(),
        crate::target::instance(),
        ds.uid,
        ds.revision,
        crate::cmd::file::unique_id(|_| false)
    );
    let recovery_bytes = crate::cmd::file::file_bytes(ds)?;
    if let Err(e) = write_checked(Path::new(&recovery), None, &recovery_bytes) {
        return Ok(reply("unreadable", Some(format!("recovery could not be saved: {e}"))));
    }
    if let Err(e) = guard.verify(Some(&observed)) {
        return Ok(json!({"state": "conflict", "error": e.to_string(), "recovery": recovery, "target": crate::target::of(Some(ds))}));
    }
    crate::target::validate(s, target)?;
    next.file_baseline = Some(observed.clone());
    next.mark_saved();
    next.mark_checkpoint();
    next.revision = next.revision.wrapping_add(1);
    next.external_recovery = Some(recovery.clone());
    let slot = s.docs.get_mut(s.active).ok_or(EngineError::NoDocument)?;
    *slot = next;
    Ok(
        json!({"state": "refreshed", "recovery": recovery, "target": crate::target::of(Some(slot)), "disk": crate::file_state::baseline_json(Some(&observed))}),
    )
}
