---
status: complete
verdict: SHIP
fork-base: e76ff27
upstream: 6681b20
branch: feat/merge-upstream-v0.1.76
---

# Merge upstream v0.1.76 while retaining fork behavior

## Scope

Merge all eight upstream commits through `6681b20` into fork commit `e76ff27`.
Keep the fork's custom behavior and package identity. Deliver a local, tested
merge branch. Landing on master and publishing a release remain separate steps.

## Execution

| Phase | Status |
|---|---|
| Fetch upstream and inventory fork behavior | Complete |
| Resolve conflicts in an isolated worktree | Complete |
| Verify merged behavior | Complete |
| Review resolutions and commit merge | Complete |

The worktree is `.worktrees/feat/merge-upstream-v0.1.76`. The original checkout
remains on master. Its pre-existing untracked `.huguesstack/` is untouched.

The machine globally enables tag pruning. The initial upstream fetch removed the
fork-only local `v1.0.12` tag. It was restored from origin, and this clone now sets
`remote.upstream.pruneTags=false` so future upstream fetches retain fork tags.

## Conflict decisions

| Area | Resolution and reason |
|---|---|
| Cargo manifest and lockfile | Keep `1.0.12`; upstream's `0.1.76` belongs to its release line. |
| Settings migration | Add v14→v15. Fork v14 has Jev settings; upstream v14 has docs extensions. Migrate either shape, preserve explicit values, and prevent older binaries from saving away new fields. |
| Settings HTML | Normalize CRLF to LF before a three-way merge. The functional changes merge cleanly; keep custom LLM/Jev controls and Buy Plan removal alongside docs-extension and scope controls. |
| Query engine and filters | Keep `ParsedQuery` and `RerankProvider`; integrate upstream candidate gathering and scope retry. Forced retry changes the scope without restoring raw filter syntax to ranking text. |
| MCP funnel | Carry the fork's structured filters through upstream readiness retries and read docs extensions from the live settings snapshot. Keep worktree redirection. |
| Global MCP proxy | Forward upstream's scope field to the main repository's worker while retaining progress heartbeats and the worktree notice. |
| Index pipeline | Use upstream's complete raw-edge tracking and graph healing, which replace the fork's unconditional full-rebuild recovery. Keep the fork's graph-preservation tests; explicitly simulate partial old raw-edge tables in their fixture. |
| Plan module and server handlers | Keep the plan module and proxy handlers deleted. Upstream changes to removed purchasing code do not restore it. |
| Auto-merged files | Check provider dispatch, symbol-span narrowing, storage errors/retries, package names, release workflow, and MCP session compatibility. A clean text merge is only a starting point. |

## Verification matrix

| Scenario | Expected behavior | Evidence |
|---|---|---|
| Fork v14, upstream v14, and combined settings | Persist v15; retain custom endpoint, Jev keys/provider, docs list including empty list; reload unchanged | New migration tests in `src/config.rs` |
| Custom LLM and default reranker | Preserve provider inheritance and OpenAI-compatible endpoints | Existing config and LLM tests |
| Jev request, errors, quotas, and span narrowing | Retain selection, keys, retries, fallback order, and valid line ranges | `tests/jev_wire.rs`; query/key-ring unit tests |
| Scoped MCP retrieval with Jev and external filters | Structured code scope wins over inline docs scope; ranking text strips filters; empty docs scope falls back using Jev | New `scoped_retrieval_keeps_jev_and_structured_filters` test |
| Index drift, incremental/no-change runs, recovery | Preserve unrelated calls edges and heal partial old raw-edge state | Existing upstream drift tests and retained fork regressions |
| MCP availability and progress | Answer from a usable previous index while rebuilding; preserve heartbeat behavior | Availability, readiness, progress and integration suites |
| Worktree, scope and progress across the global proxy | Spawn the main worker, forward docs scope, emit progress during a slow index, and avoid registering the worktree | New real-worker `global_mcp_preserves_worktree_scope_and_progress` test; store tests |
| Buy Plan removal | No purchasing controls or engine/router proxy endpoints return | Existing integration tests |
| Settings UI | Custom LLM/Jev controls coexist with docs scope controls and save correctly | Native Chrome exercise and reload passed; HTTP readback confirmed custom endpoint, Jev selection, separate fake key, docs extension, and version 15 |
| Packaging | Keep fork version and npm names/workflow | Manifest and release diff checks |

## Reproduce this merge

These commands describe the merge that produced this branch. Use a fresh branch
and worktree when repeating them; the delivered branch already contains the result.

```sh
git fetch --no-prune-tags upstream
git worktree add .worktrees/feat/merge-upstream-v0.1.76 -b feat/merge-upstream-v0.1.76 e76ff27
cd .worktrees/feat/merge-upstream-v0.1.76
git merge --no-commit --no-ff 6681b20
```

Resolve each conflict according to the table above. Avoid choosing an entire side
for settings, MCP, or retrieval. Normalize HTML line endings in all three inputs
before merging that file; preserve the fork's removed plan code.

Run the verification commands below, stage the resolutions, and create a merge
commit with both parents. Inspect the delivered merge with `git log -1 --format=fuller`
and `git show --format=raw --no-patch HEAD` from its worktree.

## Results

**SHIP for the behavior matrix above.** The final full-suite command exited 0.

| Check | Result |
|---|---|
| `cargo check --locked --all-targets` | PASS |
| `cargo test --locked --all-targets` | PASS: 953 reported passed, 0 failed, 19 existing ignored across 16 executables |
| Config suite | PASS: 54 tests, including both released v14 shapes and reload |
| New scoped MCP/Jev/filter integration | PASS |
| New global MCP worktree/scope/progress test | PASS using a real worker, real Git worktree, local embedding mock and the normal 15-second heartbeat |
| Settings browser exercise | PASS: save and reload custom LLM endpoint, Jev provider, fake TypeSafe key and docs extension together |
| Embedded JavaScript syntax | PASS: both embedded script blocks via `node --check` |
| Formatting | PASS: `rustfmt --check --edition 2024 --config skip_children=true` on resolution/new Rust files |
| Packaging comparison | PASS: Cargo manifests, lockfile, npm packages and release workflow exactly match `e76ff27` |
| Review | PASS: Standards 0 findings; Spec 0 findings |

The ignored tests are existing scale, machine-specific diagnostic, and opt-in worker
suites. One additional test (`repro_full_rebuild_notepad_ade_fresh_db`) reports a pass
but returns early because its Windows source fixture is absent; it supplies no
behavioral evidence here. The newly added real-worker test runs without an ignore flag.

The whole-repo `cargo fmt --all -- --check` reports pre-existing formatting in
`vendor/tree-sitter-liquid/build.rs`, benchmark sources, and an existing integration
test block. Those unrelated lines were left as they were. The imported scope-fallback
prompt intentionally ends with a blank separator; whitespace checking with
`core.whitespace=-blank-at-eof` passes.

Verification ran locally on macOS with the development binary and local provider
mocks. Cross-platform release builds and hosted provider calls were not run.
Raw logs remain at `/tmp/ace-upstream-check.log`, `/tmp/ace-upstream-tests.log`,
`/tmp/ace-upstream-config.log`, `/tmp/ace-upstream-scoped.log`, and
`/tmp/ace-upstream-proxy.log`.

## Review

### Standards

No findings. The review found no documented standards violations or actionable
baseline smells in the merge resolutions. It checked config migration, structured
filters, scope propagation, Jev/span interfaces, worktree routing, retained graph
tests, and the new real-worker compatibility test.

### Spec

No findings. The review found the approved fork behavior retained, including Jev
opt-in and separate keys, custom endpoints, filtered ranking text, worktree routing,
heartbeats, graph and storage fixes, purchasing removal, and packaging identity.

Each axis reviewed the immutable staged snapshot `b1aba1c` against `e76ff27`.
The reviewers read code and tests; test execution is reported separately above.
Standards: 0 findings. Spec: 0 findings.

## Landing and rollback

Review the branch before landing. From the original checkout, a local landing is:

```sh
git switch master
git merge --ff-only feat/merge-upstream-v0.1.76
```

This works while master is still an ancestor of the merge branch. If master has
advanced, first merge its new commits into the integration branch and repeat the
checks. Do not force the fast-forward.

Pushing master starts this repository's release workflow. No push or release was
performed during this merge.

Version 15 settings refuse to load in an older binary. Back up the settings file
before installing the new binary if binary rollback is required. This merge's tests
and browser exercise use scratch homes; they do not migrate the owner's settings.
