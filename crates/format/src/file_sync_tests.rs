//! File coordination uses real OS locks. Tests never use user documents.
use crate::file_sync::{Baseline, Guard, read, write_checked};
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};

struct Temp(PathBuf);
impl Temp {
    #[allow(clippy::unwrap_used)] // Test fixture only; production code returns errors.
    fn new(name: &str) -> Self {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("figcraft-file-sync-{name}-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("design.figcraft")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn file_sync_competing_writers_only_one_commits_from_the_same_baseline() {
    let dir = Temp::new("race");
    let path = dir.file();
    let baseline = write_checked(&path, None, b"initial").unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = [b"first".as_slice(), b"second"]
        .into_iter()
        .map(|bytes| {
            let (path, baseline, barrier) = (path.clone(), baseline.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                write_checked(&path, Some(&baseline), bytes)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let (_, now) = read(&path).unwrap();
    assert_eq!(results.into_iter().find_map(Result::ok).unwrap(), now);
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 2, "destination and persistent lock only");
}

#[test]
fn file_sync_busy_lock_is_not_removed_and_releases_on_drop() {
    let dir = Temp::new("busy");
    let path = dir.file();
    let guard = Guard::acquire(&path).unwrap();
    assert!(write_checked(&path, None, b"other").unwrap_err().to_string().contains("busy"));
    assert!(!path.exists());
    let saved = guard.replace(None, b"mine").unwrap();
    drop(guard);
    write_checked(&path, Some(&saved), b"next").unwrap();
}

#[test]
fn file_sync_read_baseline_matches_exact_bytes_and_same_size_changes_conflict() {
    let dir = Temp::new("bytes");
    let path = dir.file();
    write_checked(&path, None, b"aaaa").unwrap();
    let (bytes, baseline) = read(&path).unwrap();
    assert_eq!(baseline, Baseline::of(path.canonicalize().unwrap(), &bytes));
    std::fs::write(&path, b"bbbb").unwrap();
    assert!(write_checked(&path, Some(&baseline), b"cccc").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"bbbb");
}

#[test]
fn file_sync_failed_save_keeps_original_and_baseline_usable() {
    let dir = Temp::new("failure");
    let path = dir.file();
    let baseline = write_checked(&path, None, b"original").unwrap();
    let other = dir.0.join("different.figcraft");
    assert!(write_checked(&other, Some(&baseline), b"wrong path").is_err());
    assert!(!other.exists());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    write_checked(&path, Some(&baseline), b"valid").unwrap();
}

#[cfg(unix)]
#[test]
fn file_sync_symlink_and_hardlink_aliases_fail_closed() {
    use std::os::unix::fs::symlink;
    let dir = Temp::new("aliases");
    let path = dir.file();
    let baseline = write_checked(&path, None, b"keep").unwrap();
    let alias = dir.0.join("alias.figcraft");
    symlink(&path, &alias).unwrap();
    assert!(write_checked(&alias, Some(&baseline), b"wrong").is_err());
    std::fs::remove_file(&alias).unwrap();
    std::fs::hard_link(&path, &alias).unwrap();
    assert!(write_checked(&alias, Some(&baseline), b"wrong").is_err());
    assert!(write_checked(&path, Some(&baseline), b"wrong").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn file_sync_parent_symlink_retarget_is_not_treated_as_a_new_file() {
    use std::os::unix::fs::symlink;
    let dir = Temp::new("parent");
    let (a, b, alias) = (dir.0.join("a"), dir.0.join("b"), dir.0.join("alias"));
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    symlink(&a, &alias).unwrap();
    let path = alias.join("design.figcraft");
    let baseline = write_checked(&path, None, b"keep").unwrap();
    std::fs::remove_file(&alias).unwrap();
    symlink(&b, &alias).unwrap();
    assert!(write_checked(&path, Some(&baseline), b"wrong").is_err());
    assert_eq!(std::fs::read(a.join("design.figcraft")).unwrap(), b"keep");
    assert!(!b.join("design.figcraft").exists());
}

#[cfg(unix)]
#[test]
fn file_sync_preexisting_symlink_lock_is_not_followed() {
    use std::os::unix::fs::symlink;
    let dir = Temp::new("lock-symlink");
    let victim = dir.0.join("innocent");
    std::fs::write(&victim, b"keep").unwrap();
    symlink(&victim, dir.0.join("design.figcraft.figcraft-lock")).unwrap();
    assert!(write_checked(&dir.file(), None, b"no").is_err());
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    assert!(!dir.file().exists());
}

// A separate process must observe the parent's lock, not just a process-local mutex.
#[test]
fn file_sync_cross_process_lock() {
    const CHILD: &str = "FIGCRAFT_FILE_SYNC_TEST_CHILD";
    if let Some(path) = std::env::var_os(CHILD) {
        assert!(write_checked(&PathBuf::from(path), None, b"child").unwrap_err().to_string().contains("busy"));
        return;
    }
    let dir = Temp::new("process");
    let guard = Guard::acquire(&dir.file()).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "file_sync_tests::file_sync_cross_process_lock", "--nocapture"])
        .env(CHILD, dir.file())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    assert!(!dir.file().exists());
    drop(guard);
    write_checked(&dir.file(), None, b"parent").unwrap();
}
