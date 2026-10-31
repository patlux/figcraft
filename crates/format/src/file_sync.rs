//! Optimistic file replacement for participating FigCraft writers.
//!
//! A baseline identifies the exact bytes read, not an mtime. Writers hold a short-lived OS lock
//! on a persistent sibling file while they revalidate the baseline and atomically replace the
//! destination. Never unlink lock files: a waiter could still hold the old inode. OS locks release
//! on process exit, so an empty leftover lock file is normal, not a stale reservation.
//!
//! This coordinates writers using this API on a local filesystem with working advisory locks.
//! It cannot prevent a writer that ignores the lock from racing the final check/rename. It is not
//! a durability guarantee across power loss, nor a distributed/network-filesystem lock protocol.

use sha2::{Digest, Sha256};
use std::{io, path::PathBuf};

/// Bound disk reads, including files that grow after their metadata was checked.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Baseline {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

impl Baseline {
    pub fn of(path: PathBuf, bytes: &[u8]) -> Self {
        Self { path, sha256: format!("{:x}", Sha256::digest(bytes)), bytes: bytes.len() as u64 }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{Baseline, MAX_FILE_BYTES, PathBuf, io};
    use std::{
        fs::{File, OpenOptions},
        io::Read,
        path::Path,
    };

    fn conflict(message: &str) -> io::Error {
        io::Error::other(format!("file conflict: {message}; preserve both versions and save a separate copy"))
    }

    /// One pathname per lock. Final symlinks and multiply-linked files are refused rather than
    /// giving aliases independent locks. Parent symlinks resolve to their canonical directory.
    pub fn target(path: &Path) -> io::Result<PathBuf> {
        let name = path.file_name().ok_or_else(|| io::Error::other("file path has no name"))?;
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
        let absolute = parent.canonicalize()?.join(name);
        match std::fs::symlink_metadata(&absolute) {
            Ok(meta) => {
                if !meta.is_file() || meta.file_type().is_symlink() {
                    return Err(conflict("destination is not a regular, non-symlink file"));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if meta.nlink() != 1 {
                        return Err(conflict("multiply-linked destinations are not supported"));
                    }
                }
                absolute.canonicalize()
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(absolute),
            Err(e) => Err(e),
        }
    }

    fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
        let file = File::open(path)?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return Err(io::Error::other("not a regular file of at most 512 MiB"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(io::Error::other("file exceeds 512 MiB"));
        }
        Ok(bytes)
    }

    /// Read once: the baseline hashes exactly the bytes the caller will parse.
    pub fn read(path: &Path) -> io::Result<(Vec<u8>, Baseline)> {
        let absolute = target(path)?;
        let bytes = read_bytes(&absolute)?;
        if target(path)? != absolute {
            return Err(conflict("path changed while reading"));
        }
        let baseline = Baseline::of(absolute, &bytes);
        Ok((bytes, baseline))
    }

    /// A short-lived lock, scoped to one save or refresh. Do not retain it while editing.
    pub struct Guard {
        requested: PathBuf,
        path: PathBuf,
        _lock: File,
    }

    impl Guard {
        pub fn acquire(path: &Path) -> io::Result<Self> {
            let absolute = target(path)?;
            let mut lock_name = absolute.as_os_str().to_os_string();
            lock_name.push(".figcraft-lock");
            let lock_path = PathBuf::from(lock_name);
            let lock = match OpenOptions::new().read(true).write(true).create_new(true).open(&lock_path) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    // Never follow a pre-existing symlink, or write any contents to a lock file.
                    let meta = std::fs::symlink_metadata(&lock_path)?;
                    if !meta.is_file() || meta.file_type().is_symlink() {
                        return Err(conflict("invalid lock file"));
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        if meta.nlink() != 1 {
                            return Err(conflict("multiply-linked lock file"));
                        }
                    }
                    OpenOptions::new().read(true).write(true).open(&lock_path)?
                }
                Err(e) => return Err(e),
            };
            lock.try_lock().map_err(|e| io::Error::other(format!("file busy: another FigCraft writer holds the lock ({e})")))?;
            if target(path)? != absolute {
                return Err(conflict("path changed while locking"));
            }
            Ok(Self { requested: path.to_path_buf(), path: absolute, _lock: lock })
        }

        /// Check under the lock. None means create a new file, never overwrite an unseen one.
        pub fn verify(&self, expected: Option<&Baseline>) -> io::Result<()> {
            if target(&self.requested)? != self.path {
                return Err(conflict("destination path changed"));
            }
            match (read_bytes(&self.path), expected) {
                (Ok(bytes), Some(old)) if Baseline::of(self.path.clone(), &bytes) == *old => Ok(()),
                (Err(e), None) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                (Ok(_), None) => Err(conflict("destination already exists and has no observed baseline")),
                (Ok(_), Some(_)) => Err(conflict("file changed on disk since it was opened or saved")),
                (Err(e), Some(_)) if e.kind() == io::ErrorKind::NotFound => Err(conflict("original file is missing")),
                (Err(e), _) => Err(e),
            }
        }

        /// Replace the file whatever it holds now, for a caller that explicitly asked to
        /// overwrite (local fork: `file.save {overwrite: true}`). Still under the writer lock.
        pub fn overwrite(&self, bytes: &[u8]) -> io::Result<Baseline> {
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(io::Error::other("file exceeds 512 MiB"));
            }
            if target(&self.requested)? != self.path {
                return Err(conflict("destination path changed"));
            }
            crate::write_file_atomic(&self.path, bytes)?;
            Ok(Baseline::of(self.path.clone(), bytes))
        }

        pub fn replace(&self, expected: Option<&Baseline>, bytes: &[u8]) -> io::Result<Baseline> {
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(io::Error::other("file exceeds 512 MiB"));
            }
            self.verify(expected)?;
            crate::write_file_atomic(&self.path, bytes)?;
            Ok(Baseline::of(self.path.clone(), bytes))
        }
    }

    pub fn write_checked(path: &Path, expected: Option<&Baseline>, bytes: &[u8]) -> io::Result<Baseline> {
        Guard::acquire(path)?.replace(expected, bytes)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{Guard, read, target, write_checked};

#[cfg(target_arch = "wasm32")]
pub fn read(_path: &std::path::Path) -> io::Result<(Vec<u8>, Baseline)> {
    Err(io::Error::other("no file system in the browser"))
}

#[cfg(target_arch = "wasm32")]
pub fn target(_path: &std::path::Path) -> io::Result<PathBuf> {
    Err(io::Error::other("no file system in the browser"))
}

#[cfg(target_arch = "wasm32")]
pub fn write_checked(_path: &std::path::Path, _expected: Option<&Baseline>, _bytes: &[u8]) -> io::Result<Baseline> {
    Err(io::Error::other("no file system in the browser"))
}
