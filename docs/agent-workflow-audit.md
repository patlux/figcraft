# Local agent workflow audit

Date: 2026-10-16. Scope: the local `fix-agent-document-sync` branch and its paired Python bridge and agent skill.
The source base is fork `76c4e669e62f984c63be24aa07fc3aaa05e9680e` (upstream `eb741a3`).
The installed app and CLI still use that base. This audit does not claim installed synchronization or Figma parity.

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
| Refresh external updates in hidden documents | Monotonic round-robin desktop poller. Hidden-window tests and native minimized smoke pass. | Native files only. One tab per second. Synchronous large-file work can stall the UI. |
| Keep conflicting work | Dirty, missing, invalid and preview cases preserve live/disk state. Conflicting saves fail. Native smoke checks unchanged external bytes. | No automatic merge. User must resolve genuine competing edits. |
| Keep recoverable history | Adoption keeps both histories and a complete prior archive. Refresh keeps prior history in recovery and loads incoming history. Undo remains available. | Recovery files are not pruned. Undo changes live state, not disk, until a later save. |
| Verify what the window displays | Native smoke captures and checks three windows. Reviewed images show refresh, adoption and preserved conflict. | Async canvas can display an old texture briefly after the title changes. Status is not a pixel-ready signal. |
| Give agents correct operating instructions | Central routing, dedicated skill, MCP schemas and infrastructure operating docs distinguish source, package, installation and processes. | Paired activation and installed integration remain separate work. |
| Avoid silent invalid targets | Selected upstream #675/#608 changes reject malformed parent/layer IDs and invalid document indices. Seven regressions failed before the fix. | This does not make every legacy parameter parser strict. |

## Verification summary

- Full workspace after selected upstream guards: **2,400 passed, one failed, eight ignored**.
- The known failure remains `credits::ui_tests::every_column_fits_the_about_window`: 648 pt required, 640 available.
- All ten selected ID/index regressions pass. Three compatibility cases already passed before the fix.
- Workspace clippy with warnings denied, formatting, layering and asset checks pass.
- The WASM check cannot reach project code because the local toolchain lacks `wasm32-unknown-unknown` (`core`/`std`).
- The bridge suite passes 19 tests. The infrastructure MCP check passes 12 configuration tests and two host-peer tests.
- The native smoke passes with three reviewed captures in `/tmp/figcraft-native-sync-smoke-20261016-g`.
- The skill prose checker reports zero findings.

The full workspace is not green. No test was removed or weakened.
The native example uses `NoServices` and direct Engine commands, with no control listener or user preferences.
It is not an end-to-end test of Pi, the bridge, the packaged CLI and the installed app together.
See `/Users/patwoz/dev/Personal/thirdparty/figcraft/docs/file-synchronization.md` for commands, retained artifacts and failed intermediate attempts.

## Selective upstream review

The GitHub PR API and staged patches were checked again on 2026-10-16.
The reviewed upstream main remains `23bdebd97336714ded6e826dfa8d468c9ee20e34` for this assessment.
No wholesale PR or main-branch merge is part of this change.

| PR | Checked head and state | Decision |
|---|---|---|
| [#675](https://github.com/storytold/figcraft/pull/675) | `9daf39e24ce55e369bdb3e1949927f72c18afd91`, open | Reuse parent/index validation and required-layer checks with its regression cases. Simplify redundant null matches because the existing `get` helper excludes null. |
| [#608](https://github.com/storytold/figcraft/pull/608) | `90d75218400ed4281216d6f7bcd6b293793c99a5`, merged | Reuse the close-index check. Add its regression beside the target-parameter cases, including malformed strings. |
| [#673](https://github.com/storytold/figcraft/pull/673) | `0f4c79b629b23d6a2bc792387c83d40d1dc639a4`, open | Do not import unchanged. Grouped CLI scripts resolve `$step`, but document rollback cannot undo earlier explicit file writes. Its broad all-or-nothing wording is unsafe for arbitrary scripts. Keep `$step` inside supported command batches for now. |
| [#593](https://github.com/storytold/figcraft/pull/593) | `2e819569b621d0819c42d86fc347668cb965d08b`, open | Defer agent presence, client installers, app MCP and automatic saves. `create_tree`, outline and layout lint are useful optional follow-ups, not synchronization prerequisites. No parallel replacements are added here. |
| [#572](https://github.com/storytold/figcraft/pull/572) | `98475ea664783979949f561b05d5c5ab07c2fcb0`, open | Defer preference-driven listener setup and port 7979. Keep the existing managed route rather than adding another listener. |
| [#601](https://github.com/storytold/figcraft/pull/601) | `1f6010e07c20bbff499bc2694f7e847a2b2a1ce3`, merged | Deferred-import UID/page binding is relevant to its new drop queue. This base has no such queue. Do not import the broader drag/drop and base64-open feature for file synchronization. |
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
The synchronization implementation and local evidence are ready for review. Release readiness still includes the known test and WASM limitations above.
