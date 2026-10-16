# Local file synchronization

Status: save-conflict protection implemented in the local task branch (2026-10-16).
Automatic external refresh, guarded MCP document binding and generator adoption are not yet implemented.
No package or running installation changes as part of this source work.

## Protected saves

Native document opens record a SHA-256 baseline from the exact bytes the parser receives.
`file.save`, `file.saveAs` and background `FileSnapshot::save` compare that baseline before replacing the file.
The CLI's native script output uses the same save path. Native JSON output uses the same coordination API.

A missing, changed or inaccessible original produces an error instead of a replacement.
The live document, unsaved changes and undo history remain available.
A new destination must not exist. Save As does not overwrite an unrelated existing file, even after a file-picker confirmation.
Use a separate path to preserve both versions. There is no force-overwrite option in this first slice.

The CLI can update its explicit `--in` file through `--out` when the baseline still matches.
A conversion or a new generator document has no baseline for an existing output, so that output is refused.
This is a compatibility change for scripts that repeatedly wrote the same candidate filename.
Generate unique candidate paths instead. Existing app windows do not refresh automatically yet.

## Coordination and limits

Participating writers acquire a nonblocking OS lock on a persistent sibling file named `<destination>.figcraft-lock`.
They revalidate the expected baseline under the lock, then use the existing exclusive staging file and atomic rename.
The OS releases the lock when its handle closes or the process exits.
The empty lock file stays in place. Do not delete it during use: another writer can still hold its inode.
A busy file produces an error instead of waiting indefinitely.

The protocol applies to local filesystems with working advisory locks.
It does not stop older FigCraft builds or arbitrary programs that ignore the lock.
A nonparticipating writer can still race the final comparison and rename.
Atomic rename prevents partial file contents. It does not guarantee durability across a power cut.
Network filesystem locking and distributed synchronization are not verified.

Tracked paths reject final symlinks and multiply-linked files on Unix.
Parent symlinks resolve to a canonical directory, and path changes invalidate the baseline.
Disk reads and generated native outputs have a 512 MiB limit.
The archive loader retains its existing decompression and member limits.
The generic low-level `write_file_atomic` API remains unguarded for callers without a document baseline.
Exports and recovery copies are not a substitute for a tracked document save.

## Evidence

- Four Engine conflict regressions fail on the previous implementation and pass with the fix.
- Five Engine tests cover normal save, background save, missing/corrupt originals, Save As and repeated saves after open.
- Eight format tests cover exact-byte baselines, competing threads, separate processes, busy locks, path aliases and invalid lock files.
- A CLI regression checks native and JSON output refusal, preservation of existing bytes and explicit input/output updates.
- Workspace: 2,372 passed, one failed, eight ignored.
- The existing Contributors-table test still fails at 648 pt required versus 640 pt available. It was not changed.
- Workspace clippy with warnings denied, formatting, layering and asset checks pass.
- WASM checking stops before project code because the local toolchain lacks `wasm32-unknown-unknown` (`core`/`std`).
- A timed-out workspace build left invalid incremental objects. Workspace-scoped `mbx clean` followed by rebuilding resolved the linker errors.

These checks use synthetic files. There is no live UI synchronization or screenshot evidence for this slice.
The save guard is a prerequisite, not completion of the full file/window synchronization work.
