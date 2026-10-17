# Local file synchronization

Status: slim local fork on upstream `0a01188` (2026-10-17). It keeps protected saves, guarded control targets, `file.status` and an explicit `file.refresh`.
The app does not watch or poll files. An agent calls `file.refresh` after an external write.
Generator adoption (`file.adopt`, `file.operationStatus`) and automatic polling are removed. See [Removed features](#removed-features).

## Protected saves

Native document opens record a SHA-256 baseline from the exact bytes the parser receives.
`file.save`, `file.saveAs` and background `FileSnapshot::save` compare that baseline before replacing the file.
The CLI's native script output uses the same save path. Native JSON output uses the same coordination API.

A missing, changed or inaccessible original produces an error instead of a replacement.
The live document, unsaved changes and undo history remain available.
A new destination must not exist. Save As does not overwrite an unrelated existing file, even after a file-picker confirmation.
Use a separate path to preserve both versions. There is no force-overwrite option.

The CLI can update its explicit `--in` file through `--out` when the baseline still matches.
A conversion or a new generator document has no baseline for an existing output, so that output is refused.
Generate unique output paths, or write an open file only as described in [External writes](#external-writes).

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

## External writes

The app reads a file only when it opens, saves or explicitly refreshes it. It does not detect external changes by itself.
Use this procedure when an external program must change a file that is open in the app:

1. Call `file_status` for the open document. Require `dirty: false`.
2. Write the file with the external program.
3. Call `refresh_file` with a fresh `target` from `file_status` or `inspect_document`.
4. Require `state: refreshed`, then call `file_status` and require `savedAndCurrent: true`.

If the document has local edits, `refresh_file` reports `conflict` and keeps both sides.
A later save of the stale live document is refused, because the disk no longer matches its baseline.
Save the live side to a new path before any user-authorized resolution.

## Explicit refresh

`file.refresh` (MCP `refresh_file`) loads a changed native `.figcraft` file into the same open document.
It requires a target from `file.status` or `document.inspect`. A stale target is refused.
It runs only when the document is clean and noninteractive.
Pending raw input, UI fields, dialogs and queued file imports in any window refuse the call.
Local edits produce a conflict. Missing, invalid, unreadable and path-changed files remain untouched.
Collaboration, previews and open branch tabs refuse replacement too.

Refresh retains a complete prior recovery file and one undo step.
It installs the external versions and branches, preserving document UID, page where possible and view.
Old version and branch history remain in the recovery file rather than in the newly loaded file.
The external file is never rewritten during refresh. No viewport receives a focus command.
`document.inspect.lastExternalRecovery` names the retained prior file.

Engine regressions cover clean refresh, history retention, undo, local conflicts, corrupt and missing files, and previews.
A hidden-window test checks refresh without a frame or a focus change, and a retained local conflict.
Another window test checks that queued imports in the main window or another window refuse the call.

Limits: refresh reads, parses and installs the file synchronously on the session thread.
The file-size bound limits bytes, not latency. A large document can stall the UI during that call.
Recovery files are retained and are not automatically pruned.
Only native `.figcraft` originals refresh. Other formats need a separate native file.

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
Tracked reads, writes and locks go through the engine's automation roots (#715), like every other engine file access.
A path outside the roots is refused before any lock file or recovery copy is created.
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

UI regressions cover target changes, queued hidden-window calls and stale screenshot delivery.
A headless regression checks revisions, document changes and refusal of app window targets.
These are synthetic dispatcher and transport tests, not a live app demonstration.

## Removed features

The fork removed these features on 2026-10-17:

- Automatic polling. The app checked one open tab per second, including hidden tabs.
  The installed `8a59c3a` build read and hashed the whole file on each turn.
  A 211 MB document kept the main thread about 56% busy, with stalls of over one second.
  A metadata precheck in `9cf8944` reduced that cost, but kept a permanent background task with its own limits.
- Generator adoption (`file.adopt`, MCP `adopt_file`) and its receipts (`file.operationStatus`, MCP `file_operation_status`).
  The explicit procedure in [External writes](#external-writes) replaces it.
  It has no separate candidate file, no operation receipt and no recovery copy of the prior file before the external write.
  `refresh_file` still keeps a recovery copy of the prior live document.

## Native smoke test

The example uses the real eframe/WGPU event loop and `FigApp` with `NoServices`.
It creates synthetic files in a new evidence directory. It opens no listener and loads no preferences or recovery files.
Run from `/Users/patwoz/dev/Personal/thirdparty/figcraft`:

```sh
mbx run --locked -p figcraft --example sync_smoke -- /tmp/figcraft-sync-new-run
```

The absolute evidence directory must not exist. The example has a 30-second deadline and returns nonzero on failure.
It minimizes its own window, changes the file externally and refreshes it explicitly before another UI frame draws.
Then it restores the window, refreshes a second external change and introduces a local/external conflict.
A refused conflicting save must leave the external bytes unchanged.
Three window captures must contain the expected canvas colors. The native window needs enough visible space to draw those captures.

The renderer completes asynchronously. A new document title can appear before the new canvas pixels.
The example waits two seconds before capture and checks the captured pixels.
This proves native behavior for the synthetic fixture, not large-file latency or Figma parity.
The example calls Engine commands directly. It does not test the installed app, bridge relay or packaged CLI together.
