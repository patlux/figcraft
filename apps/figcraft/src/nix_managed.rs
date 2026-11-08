//! Fork: a copy that Nix installed is updated by Nix, not by the app.
//!
//! Its bundle lies in the read-only, root-owned `/nix/store` under the release's bundle id, so the
//! updater takes it for a release copy it may replace: the check at start offers every newer
//! release, and "Update and restart" would copy the release's bundle over one it cannot write (or,
//! where it could, swap the fork for upstream's build until the next activation). Such a copy skips
//! the check at start; Check for updates in the ? menu still asks when someone wants to know.

use std::path::Path;

/// Whether the executable at `exe` (resolved) is one that Nix installed: inside `/nix/store`.
pub fn installed_by_nix(exe: &Path) -> bool {
    exe.starts_with("/nix/store")
}

/// Whether the app asks for a newer release when it starts: not when Nix installed it.
pub fn startup_update_check() -> bool {
    match std::env::current_exe() {
        Ok(exe) => !installed_by_nix(&exe.canonicalize().unwrap_or(exe)),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_in_the_nix_store_is_nix_s_to_update() {
        let store = Path::new("/nix/store/dx98z9fj-figcraft-fork-0f99136/Applications/FigCraft.app/Contents/MacOS/FigCraft");
        assert!(installed_by_nix(store));
        for elsewhere in [
            "/Applications/FigCraft.app/Contents/MacOS/FigCraft",
            "/Users/someone/Applications/FigCraft.app/Contents/MacOS/FigCraft",
            "/Users/someone/nix/store/x/FigCraft",
            "/nix/storefront/FigCraft",
            "relative/nix/store/FigCraft",
        ] {
            assert!(!installed_by_nix(Path::new(elsewhere)), "{elsewhere}");
        }
    }

    #[test]
    fn a_build_outside_the_store_still_checks_at_start() {
        // This test binary runs from the Cargo target directory.
        assert!(startup_update_check());
    }
}
