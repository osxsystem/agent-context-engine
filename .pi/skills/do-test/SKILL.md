---
name: do-test
description: "Test a change and return a verdict backed by evidence: new-feature verification or bug-fix regression. Use when the user wants a feature tested, a fix verified, a suite or coverage run, or UI behaviour checked; or when another skill needs testing after implementation."
category: utilities
keywords: [test, verdict, evidence, regression, coverage, e2e]
argument-hint: "[what to test] OR ui [url]"
metadata:
  author: osxsystem
  version: "2.0.0"
---

# Testing: Verdict by Evidence

A run ends in a **verdict** on the change, and every verdict cites **evidence** you observed: a command, its output, a surface you read. A green suite is evidence about the suite; it becomes evidence about the change only once the matrix shows the suite covers it.

**Green means the code works.** A test earns green through a root-cause fix; its assertions, mocks, and skips end the run as strict as they started.

## Branch

| Invoked with | Ground on | Phase 2 derives |
|---|---|---|
| nothing | `AskUserQuestion`, header "Test scope", offering a code-level run or a `ui` run | None |
| a feature, plan, or "test X" | acceptance criteria + the diff | rows over every stated behaviour |
| a bug fix (ticket, "verify the fix", post-`cook` fix) | bug report + fix diff | the **red** reproduction + the fix's **blast radius** |
| `ui [url]` | `references/ui-surface.md` | rows over pages, flows, viewports |

## 1. Ground

Establish what changed and what correct means, before designing a single test.

- Read the trigger: ticket, acceptance criteria, plan, bug report.
- Take the diff (`git diff`, `git log`, branch vs merge-base) and name every changed file and symbol.
- Give each changed behaviour an **oracle**: the source that decides correct: acceptance criteria, the ticket's expected result, an existing contract or test, the caller's usage. Where a behaviour has no oracle, state your assumption and carry it into the report's open questions.
- For a bug fix, add the mechanism: what the fix changed, and why that ends the bug. A fix you cannot explain is a fix you cannot verify.

**Done when:** every changed symbol has a stated expected behaviour and a named oracle.

## 2. Matrix

Design the cases before running anything. `references/test-matrix.md` covers case classes, boundary heuristics, blast-radius derivation, coverage thresholds.

Each row: `scenario | input | expected | surfaces | source (existing test / to write / manual)`.

Per behaviour, take a row from each class that applies: happy path, boundaries, error and invalid input, and, where the change touches shared state, ordering and concurrency. Then extend along the **blast radius**: the callers, shared state, contracts, and adjacent behaviours the change can reach. Size the matrix to that reach; a copy tweak and a payment path do not earn the same matrix.

<HARD-GATE>
A bug fix carries one mandatory row: the reproduction from the report, run against the pre-fix code (`git stash`, or revert the changed file) and seen **red**, then run against the fix and seen green. A reproduction that never went red proves nothing about the fix, so report UNVERIFIED and name the step that failed to produce red.
</HARD-GATE>

**Done when:** the matrix is written and every behaviour from Phase 1 appears in at least one row.

## 3. Run

- Preflight the repo's own gate, meaning typecheck, lint and build, so compile errors surface as compile errors.
- Discover commands from the repo: `package.json` scripts, Makefile, Gradle tasks, `pyproject.toml`, the CI workflow. The repo's command beats a remembered one.
- Order cheapest and most specific first: the rows targeting the change, then the affected module's suite, then the full suite.
- Isolate: fresh seeded state per row, nothing carried between rows. Anything that writes outside the repo runs in a scratch working folder.
- Capture raw output for every command. That output is the evidence.
- A row with no automated test gets exercised directly, by script, REPL, CLI call, HTTP request, or UI, or is carried forward as UNVERIFIED with the reason.

**Done when:** every row has captured output, or an UNVERIFIED reason.

## 4. Verify

Exit code is one **surface**. Read every surface the change touches:

| Surface | Read for |
|---|---|
| Return value / API response | shape, status code, error payload |
| Persisted state | rows written, updated, deleted; migrations applied; no orphans |
| Logs & telemetry | expected events emitted; log clean of new errors and warnings |
| Filesystem & artifacts | files produced, temp data cleaned up |
| UI | rendered result against expectation, per `references/ui-surface.md` |
| Timing | duration against the pre-change baseline, on hot paths |

Audit the greens: a pass counts when the test ran and asserted. Confirm the expected test count, account for every skip, and where a pass looks too easy, break the behaviour briefly and watch the test go red; details are in `references/triage.md`.

**Done when:** every row carries PASS, FAIL, or UNVERIFIED, plus the evidence that decided it.

## 5. Triage & report

Every FAIL goes through `references/triage.md`: classify it (environment, test, data, non-determinism, or product code), form a hypothesis, cut it to the smallest reproduction, and confirm the cause by making the failure appear and disappear on one change. A cause you have not made flip is a guess, and reports as one.

Write the report with `references/report-format.md`; path and naming from the `project-organization` skill. Lead with the verdict:

- **SHIP**: every row PASS
- **BLOCKED**: one or more FAIL, each with a confirmed cause
- **UNVERIFIED**: coverage gaps, or a fix whose reproduction never went red

Hand back the diagnosis and the recommended fix; the caller decides whether you implement it.

**Done when:** the report exists, every FAIL carries a confirmed cause and reproduction steps, and every UNVERIFIED row names what would close it.

## Team mode

As a teammate: claim the next unblocked task from the **project's own tracker**, the tool the repo's agent instructions name (in this fork's repos: beads, via `bd`); the harness Task tools (`TaskList`/`TaskGet`/`TaskUpdate`) only where the project names none. Read the task in full before starting, wait out blocked implementation tasks, edit only the test files you own, and close by marking the task complete in that tracker plus a `SendMessage` of the verdict to the lead.

## Workflow position

**Follows:** `cook` Step 4, or a bug fix. **Precedes:** `code-review`.
