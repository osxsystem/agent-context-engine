# Codebase retrieval test report — 2026-09-14

**Verdict: UNVERIFIED for the reported client failure.** All four live retrieval calls and all 49 automated tests passed. No failure reproduced; no product code changed and no confirmed fix recommended.

## Under test

Merge `8a520a8`, upstream `e03da10..0c1f1b9`. Local release server PID 51541 on `127.0.0.1:6699`, started at 08:48:07 after the release binary timestamp 08:46:42. Server reports version 1.0.0.

Changed files: `src/mcp.rs`, `src/mcp/progress.rs`, `tests/mcp_session_restore.rs`. Both codebase and file retrieval handlers gain a heartbeat wrapper; helper covers token selection, concurrent ticks, cancellation and notification failure. The fork global endpoint uses a separate proxy handler.

Oracle: MCP initialization and tool listing succeed, tools/call has a matching response ID without JSON-RPC error or isError, and retrieval returns source snippets with line references. Heartbeat tests assert result preservation, cancellation, send failure and session keep-alive behavior. Exact user client/error/query remain unknown.

## Matrix

| Scenario | Expected | Result | Evidence |
|---|---|---|---|
| Repository endpoint, OF1 web repo, explicit progress token | Successful retrieval and matching progress token | PASS | 22.37 seconds, notification token `repro-3`, snippets from `src/main.jsx` and `vite.config.js` |
| Repository endpoint, OF1 web repo, no progress token | Successful retrieval without requiring token | PASS | 28.78 seconds; source snippets returned; not a claim that no-token notifications reached the POST stream |
| Global endpoint, OF1 web repo, explicit token | Workspace routed correctly; source snippets | PASS | 24.10 seconds; no progress notification observed in POST stream |
| Repository endpoint, engine repo, explicit token | Successful retrieval and progress during wait | PASS | 46.76 seconds, three matching notifications; `current-repo.log` |
| Initialization, initialized notification, tool discovery | HTTP 200 / 202 / 200; codebase-retrieval listed | PASS | All four live sessions |
| MCP module tests | Heartbeat, readiness, query-gate and retrieval helper contracts | PASS | `unit-tests.log`: 46 passed, 0 failed, 0 ignored; 612 filtered out |
| Session tests | Unknown-session rejection, idle restoration, heartbeat survival | PASS | `session-tests.log`: 3 passed, 0 failed, 0 ignored |
| User's exact client failure | Reproduce supplied failure | UNVERIFIED | Client, exact error, endpoint and triggering request not supplied |
| File retrieval end-to-end; >300-second retrieval; client handling of progress | No regressions | UNVERIFIED | Live tests limited to codebase retrieval using fresh Python HTTP sessions |

## Reproduction commands

From the repository root:

```bash
python3 plans/reports/260914-0916-codebase-retrieval/repro.py /Users/hugues_mini/Codes/OF1/ofone_web_reactjs repo token
python3 plans/reports/260914-0916-codebase-retrieval/repro.py /Users/hugues_mini/Codes/OF1/ofone_web_reactjs repo
python3 plans/reports/260914-0916-codebase-retrieval/repro.py /Users/hugues_mini/Codes/OF1/ofone_web_reactjs global token
python3 plans/reports/260914-0916-codebase-retrieval/repro.py /Users/hugues_mini/Codes/AgentTools/agent-context-engine repo token
cargo test --release --locked --lib mcp::
cargo test --release --locked --test mcp_session_restore
```

Script initializes a fresh MCP session, lists tools, calls codebase-retrieval with “Where is the application entry point and how does it start the server?”, prints notifications and a bounded response excerpt, and asserts successful source content. It uses the running server and existing indexes/configuration; live timing and ranking are not deterministic. Real retrieval may call configured external embedding/reranking services and trigger normal index maintenance. Script is a diagnostic harness, not a regression test for an identified root cause. No-token and global runs overlapped; this does not constitute a concurrency stress test.

## Build and coverage

Release test builds passed. Existing bundled Liquid scanner unused-parameter warnings remain. Unit execution: 0.10 seconds; session execution: 2.21 seconds, excluding compilation. No line/branch coverage collected, full suite not run. Passing helper tests do not establish compatibility with the user's unidentified MCP client. Runtime process logs went to the user's terminal and were not captured; HTTP/SSE responses were inspected directly.

## Recommendations and unresolved questions

1. Obtain exact client name, project path, endpoint, query and full redacted error; replay through that client. This is necessary to distinguish a client/session-specific failure from a retrieval failure.
2. Do not attribute the failure to the merge or change product code based only on these passing tests. No red/green comparison or pre-merge differential test was performed.
3. The global proxy does not use the newly merged heartbeat wrapper (`src/router/mcp_proxy.rs`); its 24-second call succeeded without a progress event in the POST stream. Long-running global requests need separate verification; this is not a confirmed cause of the reported failure.

Only diagnostic artifacts were added. Existing untracked `banks/` was left untouched.

## Follow-up — newly launched Codex instances

User clarified that the failure happens in a new Codex instance created by the main agent. Local session metadata shows fresh CLI sessions in OF1 HR worktrees. Exact failing instance and diagnostic are still unconfirmed.

**Confirmed configuration defect in a sampled worktree:**

| Test | Result |
|---|---|
| `codex mcp get codebase-retrieval` in main OF1 web checkout | PASS: enabled streamable HTTP server, URL below |
| Same command in `feat-hr-07-recruitment` worktree | FAIL: `Error: No MCP server named 'codebase-retrieval' found.` |
| Same worktree and command with only a command-line URL override | PASS: server discovered as enabled |

Main checkout configuration: `/Users/hugues_mini/Codes/OF1/ofone_web_reactjs/.codex/config.toml`. URL: `http://127.0.0.1:6699/mcp-repo/Users_hugues_mini_Codes_OF1_ofone_web_reactjs`.

All eight inspected `feat-hr-06` through `feat-hr-13` worktrees lack `.codex/config.toml`. Global `~/.codex/config.toml` also lacks the `codebase-retrieval` server. The engine checkout has no such local entry either. The sampled worktree CLI does not discover the main checkout's server configuration.

One-variable reproduction, from the recruitment worktree:

```bash
codex mcp get codebase-retrieval
codex -c 'mcp_servers.codebase-retrieval.url="http://127.0.0.1:6699/mcp-repo/Users_hugues_mini_Codes_OF1_ofone_web_reactjs"' mcp get codebase-retrieval
```

First command exits 1; second exits 0. This confirms missing effective configuration, not a retrieval transport error. `mcp get` is configuration inspection and does not perform a real Codex tools/call. Earlier HTTP tests establish that the configured endpoint can serve retrieval.

**Recommended fix:** ensure the launcher supplies this MCP server entry to each new worktree Codex instance, either through that worktree's project configuration or an explicit launch override. Use the main repository endpoint for these worktrees. Avoid globally binding unrelated repositories to the OF1-specific endpoint. Restart/reconnect the affected instance after configuration is supplied, then test an actual Codex retrieval call.

No configuration or product changes applied. Missing configuration is confirmed in the sampled worktree; whether it explains the user's exact failure remains pending confirmation of how the instance was launched. No evidence establishes the upstream merge as its cause.

## Orca confirmation

User confirmed the new Codex instance was launched in an Orca worktree. Read-only Orca CLI inspection confirmed:

- Orca 1.4.200, local execution host.
- Recruitment worktree metadata: `createdWithAgent: codex`, CLI provenance `created-by-cli`, `startupAgent: codex`.
- Terminal `term_8caea034-d8c2-4583-920d-2d1be8875073` belongs to that worktree and identifies as Codex.
- Its captured terminal output states: “This session has neither codebase-retrieval nor file-retrieval, and no MCP resources expose them.”
- Main checkout `.codex/config.toml` is ignored by `.gitignore:69` (`.codex`). It is absent in the inspected worktrees.

Commands: `orca status --json`, `orca worktree show --worktree path:<recruitment-path> --json`, `orca terminal list --worktree path:<recruitment-path> --json`, `orca terminal show --terminal <handle> --json`, and `orca terminal read --terminal <handle> --cursor 1377 --limit 1000 --json`.

**Diagnosis for the inspected instance:** missing MCP configuration prevents tool availability. Earlier one-variable CLI configuration tests support this, and the terminal now confirms the matching user-visible symptom. No actual failing retrieval request was found in this instance: the tools are unavailable before a call can be made.

**Fix to apply:** configure the codebase-retrieval server as part of Orca worktree setup before launching Codex (project config entry or launch override). For these OF1 worktrees use the existing main-checkout MCP endpoint. Restart/reconnect existing affected Codex instances after adding their configuration, preserving their sessions. Do not copy unrelated credentials or all global Codex settings.

No terminal input was sent; no agents, worktrees, or configuration were modified. Actual Codex retrieval after configuration repair remains unverified because implementation was not requested.
