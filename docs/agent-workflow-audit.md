# Local agent workflow audit

Date: 2026-10-18. Scope: the slim local fork on `main`, merged with upstream `e9565813ac724abf50f1089eaeee3960a8366ffd` (v0.7.0 and community batch 18) and with open upstream PRs, and the paired Python bridge and agent skill.
This audit does not claim installed synchronization or Figma parity.

## What the fork adds to upstream

| Fork change | Status | Why it stays |
|---|---|---|
| Protected saves: exact-byte baseline, writer lock, atomic replacement | Fork only | Without it, a save silently replaces a file that another program changed. |
| Guarded control targets: app instance, document, path, revision and window, checked at dispatch | Fork only | The bridge pins each Pi session to one document. Without it, edits can reach another tab after a tab switch or an app restart. |
| `file.status` (MCP `file_status`) | Fork only | Exact live/disk observation. It does not change anything. |
| Explicit `file.refresh` (MCP `refresh_file`) | Fork only | Loads an external write into the open document with a recovery copy and one undo step. |
| Queued imports refuse replacement in every window | Fork only | Upstream's drop queue could otherwise edit a document that a refresh replaced. |
| A window not created yet counts as hidden | Fork only | Without it, untargeted requests wait for a window that the hidden main window never creates. |
| Control requests wake the main window | Fork only | Upstream wakes the window that draws at that moment. A request during another window's frame then waited 10 s or more. |
| CPU inner-shadow blend modes | Fork only | Upstream still draws inner shadows with Normal. No current design depends on other modes. |
| Protected I/O behind upstream automation roots (#715) | Fork only | The fork's tracked reads, writes and locks use the same roots as every other engine file access. |

Removed from the fork on 2026-10-17: automatic file polling and generator adoption (`file.adopt`, `file.operationStatus`).
The poller read and hashed every open file once per turn. A 211 MB document kept the main thread about 56% busy.
See `/Users/patwoz/dev/Personal/thirdparty/figcraft/docs/file-synchronization.md` for the remaining contract.

## Open upstream PRs in the fork

The fork merges these open upstream PRs as they are, with real merge commits:
#801, #287, #285, #607, #661, #659 (with #623), #320, #618, #696 (with #748) and #538.

| From #593 | Status |
|---|---|
| `node.createTree` / MCP `create_tree`, `inspect.outline` / `outline`, `inspect.lint` / `lint_layout`, `text.measure` | Ported |
| `edit.duplicate` returns `map`, the copy of every copied layer | Ported |
| `ui.screenshot {after}`: wait up to 30 s before the capture | Ported |
| `--autosave` / `FIGCRAFT_MCP_AUTOSAVE=1` | Ported. It calls the protected `file.save`: a file that another program changed stays, and the tool result carries a note. |
| Preferences ▸ AI agents, client installer, port management, client hello | Not ported. The fork starts the control port from the LaunchAgent and installs clients through Nix. |

#659 repaints pages that still have their theme colour when the theme changes. It kept one UI test that expects the old behavior.
The fork changes that test to the new behavior. #500 (control authentication) stays separate:
it needs a token shared by the LaunchAgent, the bridge and the CLI.

## Now in upstream

- Hidden-window control requests: #332 (from this fork) and #720. The fork now uses upstream's behavior.
  `ui.input` and `window.new` to a hidden window are errors at once. The fork no longer buffers raw input for hidden windows.
- Upstream merged the parameter guards #675 and #608, the drop queue #601 and grouped scripts #673.
  The upstream code replaces the fork's selective copies.
- Finder/Dock open (#290) and long home-screen paths (#324).

## Verification summary

- The installed-chain test on 2026-10-18 found lost control-request wakes. After a document change in a hidden window,
  9 of 80 direct requests waited 2 s or more, up to over 25 s. The main thread was idle in its event loop.
  A second request released each stalled request within 0.02 s. A control-server regression fails before the fix.
- Workspace after the open-PR batch: **2,905 passed, two failed, nine ignored** (2,819 after upstream `e956581`, 2,755 after `0a01188`).
- The two failures are upstream automation-root tests on macOS:
  `file_access::tests::links_that_lead_out_of_a_root_are_outside` and `file_access::tests::outside_paths_are_refused_alike_whatever_exists_there`.
  Both expect the reason "leads nowhere" for a dangling link inside a root. On macOS the temporary folder is under `/var`, a link to `/private/var`.
  `lexically_inside` compares the unresolved path with the resolved root, so the refusal names the generic reason.

  The path is still refused. Upstream CI runs these tests on Linux only. The fork leaves the tests and the upstream code unchanged.
- A new Engine regression checks that status, refresh and save stay inside automation roots. It fails when the fork's I/O bypasses the roots.
- Workspace clippy with warnings denied, formatting, layering, assets and packaging checks pass.
- The native smoke passes with explicit refresh while minimized and visible, and with a preserved conflict.
  Three reviewed 2400×1600 captures are in `/tmp/figcraft-agent-batch-smoke-20261018`.
- The local WASM target is absent. GitHub Actions run the WASM check after a push.

The native example uses `NoServices` and direct Engine commands, with no control listener or user preferences.
It is not an end-to-end test of Pi, the bridge, the packaged CLI and the installed app together.

## Open upstream candidates

- Protected saves and guarded targets, so that the fork can shrink further.
- CPU inner-shadow blend modes.
- The window that a hidden main window has not created yet.
- The control-server wake of the main window.
- The macOS path comparison in `lexically_inside`.

Nobody submitted these yet. A pull request needs an explicit request.
