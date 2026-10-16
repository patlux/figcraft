# Local file synchronization

Status: save-conflict protection implemented in the local task branch (2026-10-16).
Guarded control targets are implemented in the local source. The paired bridge validates and pins them.
Controlled generator adoption and automatic external refresh are implemented in source.
A bounded macOS native smoke test now verifies refresh, adoption and conflict retention with window screenshots.
Installation and the configured Pi-to-bridge-to-app integration test remain pending.
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
Generate unique candidate paths instead. The desktop poller can refresh clean, noninteractive native documents.

## Current file status

`file.status` (MCP `file_status`) reads and validates the current native file without changing the live document.
It returns the exact saved-byte baseline and the current disk hash.
It distinguishes current, changed, missing, invalid, unreadable and path-changed files.
Unsaved and branch tabs have separate states.

`designMatchesDisk` compares deterministic native design fingerprints, including image bytes and published libraries.
These fingerprints exclude the selected page, thumbnail and version history.
`savedAndCurrent` also requires a clean live document and the saved-byte baseline.
Thus design equality alone does not claim that version-history edits reached disk.
The response is a point-in-time observation. It does not reserve the file or guarantee future agreement.
Status calls preserve previews, history and revision. They do not refresh the document.

Three status regressions failed before implementation. Four Engine tests now cover disk changes, errors, history, previews and image fingerprints.
An MCP regression checks the tool response and read-only annotation.

## Controlled generator adoption

Generate a separate native candidate. Never overwrite an open original.
Call `file.status` for the current target, revision and disk SHA-256.
Call `file.adopt` with that target, `candidate`, `candidateSha256`, `diskSha256` and a unique `operation` ID.
MCP exposes this as `adopt_file`.

The app requires a clean, noninteractive main document with a saved `.figcraft` baseline.
It refuses active previews, collaboration, open branch tabs and candidates with branches.
It also refuses repaired candidates, changed hashes, busy locks and full version history.
Adoption cannot run inside a rollback-capable command group.

Both participating-writer locks stay held during validation and persistence.
Before replacement, the app saves the complete previous document to `<original>.recovery-<operation>.figcraft`.
That path must not exist. A failed later step can leave the recovery copy, but does not remove it.
The app retains existing versions and candidate versions without deleting old entries.
It records the previous design and an operation receipt as named versions.
Then it atomically writes the prepared archive and installs it in the same document UID.
It preserves the view and creates one undo step. Undo does not rewrite disk until a later save.

After a lost reply, call `file.operationStatus` (MCP `file_operation_status`). Never replay automatically.
`resultPresent` requires the retained receipt, matching design, current saved baseline and disk agreement.
`changedOrUnknown` and `unknown` do not prove failure and do not authorize a retry.
Receipts survive reopen through native version history, but users can delete them.
This is outcome observation, not a durable exactly-once transaction protocol.
A crash after disk replacement but before live installation still needs restart/reopen recovery.
The existing advisory-lock and power-loss limits below apply.

Two adoption regressions failed before implementation. Four Engine tests cover recovery, both histories, undo and post-reopen outcome observation.
They also cover changed targets/files, local edits, previews, full history, busy locks, invalid candidates and recovery-write failure.
A hidden-window dispatcher test verifies in-place adoption without changing focus or another window's document.
These tests are not a demonstration in the installed native app.

## External-file refresh

The desktop checks one open tab per second, including tabs in hidden windows.
It reads exact bytes instead of trusting mtime or size alone.
The polling clock uses a monotonic wall clock: egui's UI time stops while hidden-window logic runs.
With many tabs, each tab waits for its next turn.

A changed native `.figcraft` file refreshes only when the document is clean and noninteractive.
Pending raw input, UI fields and dialogs defer replacement. Local edits produce a conflict.
Missing, invalid, unreadable and path-changed files remain untouched.
Collaboration and open branch tabs defer replacement too.
The explicit `file.refresh` command (MCP `refresh_file`) applies the same document checks.

Refresh retains a complete prior recovery file and one undo step.
It installs the external versions and branches, preserving document UID, page where possible and view.
Old version/branch history remains in the recovery file rather than silently modifying the newly loaded file.
The external file is never rewritten during refresh. No viewport receives a focus command.
`document.inspect.externalFile` exposes the last polling result, and `lastExternalRecovery` names the retained prior file.
`file.status` always makes a fresh observation.

Two Engine refresh regressions failed before implementation and now pass.
A hidden-window test checks clean refresh, local conflict and deferred-input refusal without a focus change.
Its first implementation exposed a stopped UI-clock bug. The monotonic-clock fix makes subsequent hidden polls run.
Twelve window tests pass. These are synthetic dispatcher tests, not native runtime evidence.

Current limits: polling and changed-file parsing run synchronously on the session thread.
The file-size bound limits bytes, not latency. Large documents can stall the UI.
Recovery files are retained and are not automatically pruned.
Only native `.figcraft` originals auto-refresh. Other formats need an explicit separate native candidate.

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

## Guarded control targets

Inspection exposes app-instance, document UID, path, revision and window identity.
The control dispatcher validates a supplied target immediately before execution, including hidden-window execution.
A revision is optional for identity-only calls. A stale supplied revision refuses the call.
Guarded raw `ui.input` is refused because its events are deferred to a later frame.
Legacy unguarded control clients retain their behavior. This is routing protection, not authentication.

The paired local Python bridge requires app-side guards for mutations.
On an older app, inspection remains available but mutation calls fail before forwarding.
App restart, tab change, path change or closed window invalidates the pin until explicit reselection.
Explicit window parameters cannot bypass the session reservation.
Do not activate the new bridge alone against the installed baseline if live editing must remain available.

Four UI regressions cover target changes, queued hidden-window calls and stale screenshot delivery.
The tab-switch regression failed before the change. Ten window tests now pass.
A headless regression checks revisions, document changes and refusal of app window targets.
Nineteen bridge tests pass, including five target-validation tests that failed before the fix.
These are synthetic dispatcher and transport tests, not a live app demonstration.

## Evidence

- Four Engine conflict regressions fail on the previous implementation and pass with the fix.
- Five Engine tests cover normal save, background save, missing/corrupt originals, Save As and repeated saves after open.
- Eight format tests cover exact-byte baselines, competing threads, separate processes, busy locks, path aliases and invalid lock files.
- A CLI regression checks native and JSON output refusal, preservation of existing bytes and explicit input/output updates.
- Workspace after selected upstream target guards: 2,400 passed, one failed, eight ignored.
- Ten layer/document-index regressions pass. Seven failed before the selected upstream #675/#608 changes.
- The [requirement/upstream audit](agent-workflow-audit.md) records selections, exclusions and remaining release work.
- The existing Contributors-table test still fails at 648 pt required versus 640 pt available. It was not changed.
- Workspace clippy with warnings denied, formatting, layering and asset checks pass.
- WASM checking stops before project code because the local toolchain lacks `wasm32-unknown-unknown` (`core`/`std`).
- A timed-out workspace build left invalid incremental objects. Workspace-scoped `mbx clean` followed by rebuilding resolved the linker errors.

### Native smoke test

The example uses the real eframe/WGPU event loop and `FigApp` with `NoServices`.
It creates synthetic files in a new evidence directory. It opens no listener and loads no preferences or recovery files.
Run from `/Users/patwoz/dev/Personal/thirdparty/figcraft`:

```sh
mbx run --locked -p figcraft --example sync_smoke -- /tmp/figcraft-sync-new-run
```

The absolute evidence directory must not exist. The example has a 30-second deadline and returns nonzero on failure.
It minimizes its own window, changes the file externally and verifies refresh before another UI frame draws.
Then it restores the window, adopts a separate candidate and introduces a local/external conflict.
It checks the receipt and saved state. A refused conflicting save must leave the external bytes unchanged.
Three window captures must contain the expected canvas colors. The native window needs enough visible space to draw those captures.

The reviewed run on 2026-10-16 passed in `/tmp/figcraft-native-sync-smoke-20261016-g`:

- `hiddenRefresh` and `noFramesDuringHiddenRefresh` are true.
- `adoption` and `conflictPreserved` are true. All three screenshot pixel checks pass.
- The first screenshot shows the updated purple design and text in the existing document.
- The second shows the adopted green design.
- The third preserves the green live design, marks local work unsaved and shows the conflict message.

The renderer completes asynchronously. An early capture showed the old canvas under the new document title.
Later captures showed the updated canvas without reopening. The example waits two seconds before capture and checks the captured pixels.
An intermediate run timed out before the second screenshot. Another produced a narrow window that was unsuitable for visual review.
The final example fixes its window size and refuses undersized captures. It does not change the host window manager.

This proves native behavior for the synthetic fixture, not large-file latency or Figma parity.
The example calls Engine commands directly. It does not test the installed app, bridge relay or packaged CLI together.
No package pin, installation or running user document changed.
