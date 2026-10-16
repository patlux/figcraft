# Local agent workflow audit

Date: 2026-10-16. Scope: synchronization work on local `main`, its upstream update, and the paired Python bridge and agent skill.
Local `main` includes synchronization source `4db0d6ed73397987c9e4c638421a48abf82ee82b` and upstream `d1d8c3f652cea20bee02bdbc3ded9a2f5362ba08`.
The published fork and installed app/CLI remain at `76c4e669e62f984c63be24aa07fc3aaa05e9680e` (upstream base `eb741a3`).
This audit does not claim installed synchronization or Figma parity.

## Requirement evidence

| Requirement | Implementation and evidence | Boundary |
|---|---|---|
| Recover after EOF without duplicate writes | Bridge discards dead connections and reconnects on the next explicit request. EOF, timeout and disconnect regressions pass. | An uncertain mutation is never replayed automatically. |
| Target the intended open document | Bridge checks app/window/document/path and reservation on every request. App checks identity at dispatch. Revision checks protect adoption. | Old unguarded clients remain compatible. Identity guards are not authentication. |
| Reject stale pins | Restart, tab change, path change, closed window and explicit-window bypass tests pass. Changed pins need explicit reselection. | New bridge refuses mutations on older apps without target guards. |
| Protect manual and background saves | Exact loaded-byte baseline, cooperating-writer lock and atomic replacement. Engine, CLI and cross-process format regressions pass. | Arbitrary or older writers can ignore advisory locks. No power-loss durability guarantee. |
| Report live/disk agreement truthfully | `file.status` compares exact baseline/disk bytes and design fingerprints. History-only dirtiness keeps `savedAndCurrent` false. | Point-in-time observation, not a reservation. |
| Adopt a separate generator result | `file.adopt` validates target revision and both hashes under writer locks. It keeps recovery/history and installs the same UID. | Clean native main file only. No collaboration, open branch tabs or active editing. |
| Observe an adoption after a lost reply | Retained version receipt plus current disk/live agreement. Tests discard the result, inspect, reject replay and reopen. | No durable exactly-once protocol. Receipt removal or a crash can require manual recovery. |
| Refresh external updates in hidden documents | Monotonic round-robin desktop poller. Hidden-window tests and native minimized smoke pass. Queued imports in any pane defer replacement. | Native files only. One tab per second. Synchronous large-file work can stall the UI. |
| Keep conflicting work | Dirty, missing, invalid and preview cases preserve live/disk state. Conflicting saves fail. Native smoke checks unchanged external bytes. | No automatic merge. User must resolve genuine competing edits. |
| Keep recoverable history | Adoption keeps both histories and a complete prior archive. Refresh keeps prior history in recovery and loads incoming history. Undo remains available. | Recovery files are not pruned. Undo changes live state, not disk, until a later save. |
| Verify what the window displays | Native smoke captures and checks three windows. Reviewed images show refresh, adoption and preserved conflict. | Async canvas can display an old texture briefly after the title changes. Status is not a pixel-ready signal. |
| Give agents correct operating instructions | Central routing, dedicated skill, MCP schemas and infrastructure operating docs distinguish source, package, installation and processes. | Paired activation and installed integration remain separate work. |
| Avoid silent invalid targets | Selected upstream #675/#608 changes reject malformed parent/layer IDs and invalid document indices. Seven regressions failed before the fix. | This does not make every legacy parameter parser strict. |

## Verification summary

- Full workspace after the upstream merge: **2,428 passed, zero failed, eight ignored**.
- Upstream resolves the previous `credits::ui_tests::every_column_fits_the_about_window` failure. The test now passes.
- The new queued-import regression fails before the integration fix. All 13 window tests pass after it.
- All ten selected ID/index regressions pass. Three compatibility cases already passed before the fix.
- Workspace clippy with warnings denied, formatting, layering and asset checks pass.
- The WASM check cannot reach project code because the local toolchain lacks `wasm32-unknown-unknown` (`core`/`std`).
- The bridge suite passes 19 tests. The infrastructure MCP check passes 12 configuration tests and two host-peer tests.
- The native smoke passes on the merged source, including the queued-import guard, before the merge commit.
- Three reviewed 2400×1600 captures and JSON evidence are in `/tmp/figcraft-main-upstream-smoke-20261016`.
- That run reports 231 frames, minimized refresh without UI frames, adoption, conflict preservation and three verified screenshots.
- The earlier committed-source smoke evidence remains in `/tmp/figcraft-native-sync-smoke-20261016-final`.
- Packaging identity and synthetic Caddyfile tests pass. Changed workflow YAML parses correctly. This is not remote CI evidence.
- All 39 MCP tests pass after the final initialization-instruction changes.
- Installed bundle metadata and resolved app/CLI links still select `76c4e669e62f984c63be24aa07fc3aaa05e9680e`.
- The skill prose checker reports zero findings.

The native workspace passes. WASM remains unverified because the target is absent. All tests and assertions remain in place.
The native example uses `NoServices` and direct Engine commands, with no control listener or user preferences.
It is not an end-to-end test of Pi, the bridge, the packaged CLI and the installed app together.
See `/Users/patwoz/dev/Personal/thirdparty/figcraft/docs/file-synchronization.md` for commands, retained artifacts and failed intermediate attempts.

## Upstream integration

Local `main` first fast-forwarded to synchronization commit `4db0d6e`, then merged upstream `d1d8c3f`.
The upstream update contains 45 commits, including merge commits, after the previous upstream base `eb741a3`.
It includes drop-to-open and deferred imports, canvas overscan, text/font fixes, copied-page navigation fixes and shared CI workflows.
The merge preserves the fork's hidden-window control, save guards, document targets, adoption and external refresh.
The separate renderer worktree remains outside this merge.

Upstream's deferred-import queue exposed an integration gap: refresh or adoption could replace a document before its queued edit ran.
Queued imports now block replacement in every pane, including the suspended main pane during a hidden-window call.
The regression checks polling, explicit refresh and adoption refusal, unchanged disk bytes, and refresh after the queue clears.
It also checks unchanged focus and the active document.

No open PR joins this merge as a separate feature bundle.
The requested #593 agent features remain a follow-up. This merge does not replace that feature work.
The follow-up must preserve the managed MCP route and protected save path, without automatic client installers or extra listeners.

## Selective upstream review history

The earlier review checked the GitHub PR API and staged patches on 2026-10-16.
Its reviewed upstream main was `23bdebd97336714ded6e826dfa8d468c9ee20e34`.
These decisions describe the original synchronization scope. The later main-branch merge above supersedes the deferral of merged upstream changes.

| PR | Checked head and state | Decision |
|---|---|---|
| [#675](https://github.com/storytold/figcraft/pull/675) | `9daf39e24ce55e369bdb3e1949927f72c18afd91`, open | Reuse parent/index validation and required-layer checks with its regression cases. Simplify redundant null matches because the existing `get` helper excludes null. |
| [#608](https://github.com/storytold/figcraft/pull/608) | `90d75218400ed4281216d6f7bcd6b293793c99a5`, merged | Initially reuse the close-index check with regression cases, including malformed strings. The later upstream merge also includes its ancestry. |
| [#673](https://github.com/storytold/figcraft/pull/673) | `0f4c79b629b23d6a2bc792387c83d40d1dc639a4`, open | Do not import unchanged. Grouped CLI scripts resolve `$step`, but document rollback cannot undo earlier explicit file writes. Its broad all-or-nothing wording is unsafe for arbitrary scripts. Keep `$step` inside supported command batches for now. |
| [#593](https://github.com/storytold/figcraft/pull/593) | `2e819569b621d0819c42d86fc347668cb965d08b`, open | Initially deferred during synchronization work. Agent presence, `create_tree`, outline, layout lint, duplication ID mapping and MCP autosave remain absent. Integrate the selected agent features as a separate requested follow-up, with guarded saves and the managed route. |
| [#572](https://github.com/storytold/figcraft/pull/572) | `98475ea664783979949f561b05d5c5ab07c2fcb0`, open | Defer preference-driven listener setup and port 7979. Keep the existing managed route rather than adding another listener. |
| [#601](https://github.com/storytold/figcraft/pull/601) | `1f6010e07c20bbff499bc2694f7e847a2b2a1ce3`, merged | Initially deferred because the old base had no drop queue. Now included through upstream, with UID/page-bound imports and the local replacement guard described above. |
| [#603](https://github.com/storytold/figcraft/pull/603) | `f05eecdaa0c50645b766b2b45b59b9df82d6a975`, open | Defer layout-target and font-style changes. They change multi-selection behavior and font warnings, rather than file synchronization. They need their own layout/font acceptance checks. |
| [#500](https://github.com/storytold/figcraft/pull/500) | `d58b41f272c9049b29f29c2abd13d1056052a3d7`, open | Defer mandatory token authentication. It needs a coordinated credential and launch contract across the app, CLI and relay. Document the existing unauthenticated loopback boundary instead. |
| [#670](https://github.com/storytold/figcraft/pull/670) | `c5a8420089de30cc222786efae09db9423e44967`, open | Defer the test-only shutdown adjustment. The corresponding test passes on this base, whose malformed-request lifecycle differs from newer upstream. |

The checked #593 auto-save path invokes ordinary `file.save` after a tool call.
It does not provide external-file refresh or safe adoption. The current save guard solves that conflict at the shared Engine/format boundary.
The checked candidate set contains no complete replacement for the implemented synchronization contract.
This is not a claim about every upstream branch or future revision.

## Remaining release work

1. Build and package the final app and CLI commit together.
2. Update both local package pins and artifact hashes.
3. Review the full activation plan and obtain explicit activation authorization.
4. Activate the paired app, CLI and bridge on mbpromax.
5. Start fresh MCP processes and verify the running binary identity and listener boundaries.
6. Exercise the installed chain with a new synthetic document, including stale pins, adoption, hidden refresh and conflicting saves.

Do not activate the new bridge alone if the older app must keep accepting live edits.
Package build and activation details are in `/Users/patwoz/dev/Personal/infra/nix/hosts/darwin/mbpromax/figcraft/README.md`.
The synchronization implementation and upstream merge have local native evidence. Release readiness still needs WASM verification and the installed-chain checks above.
