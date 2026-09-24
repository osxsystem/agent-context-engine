---
name: cook
description: "Cook a task end-to-end: research, plan, implement, test, review, finalize. Use when the user wants a feature implemented, a plan executed, or a fix shipped through the full pipeline."
category: utilities
keywords: [implementation, workflow, feature, pipeline]
argument-hint: "[task|plan-path] [--interactive|--fast|--parallel|--auto|--no-test] [--tdd]"
metadata:
  author: osxsystem
  version: "1.0.0"
---

# Cook: Feature Implementation Pipeline

End-to-end implementation: detect intent, research, plan, implement, test, review, finalize. Every major step ends at a **gate**, a stop for human approval, unless the mode says otherwise.

## Usage

```
/cook <natural language task OR plan path>
```

```
/cook "Add user authentication to the app" --fast
/cook path/to/plan.md --auto
/cook "Refactor auth middleware" --tdd
```

## Modes

One mode per run, selected by flag, keyword, or input shape. This table is the single source of truth for mode behavior:

| Mode | Triggers | Research | Test | Gates | Phase progression |
|------|----------|----------|------|-------|-------------------|
| **interactive** (default) | `--interactive`, or nothing else matches | ✓ | ✓ | Stops at every gate | One at a time |
| **fast** | `--fast`; "fast", "quick" | ✗ | ✓ | Stops at every gate | One at a time |
| **auto** | trailing `--auto`, or the exact phrase "trust me", and nothing else | ✓ | ✓ | None; auto-approve at zero critical review findings | All phases continuously |
| **parallel** | `--parallel`; 3+ listed features | Optional | ✓ | Stops at every gate | Parallel groups |
| **no-test** | `--no-test`; "no test", "skip test" | ✓ | ✗ | Stops at every gate | One at a time |
| **code** | Path to `plan.md` / `phase-*.md` | ✗ | ✓ | Stops at every gate | Per plan |

`--tdd` composes with any mode: write tests for current behavior first, implement, then verify those tests still pass.

Trigger cells above are representative; `references/intent-detection.md` holds the authoritative keyword lists and matching rules: whole tokens only (never substrings, so "autocomplete" is not "auto"), flags count only in trailing position, and a negated keyword is ignored.

<HARD-GATE>
A reviewed plan exists before any implementation code, regardless of task simplicity; "simple" tasks are where unexamined assumptions cost the most. `--fast` skips research, never the plan step. Only an explicit user instruction ("just code it", "skip planning") lifts this gate.
</HARD-GATE>

## Pipeline

```
Intent → Research? → [gate] → Plan → [gate] → Implement (+ conditional Simplify) → [gate] → Test? → [gate] → Review → Finalize
```

`references/workflow-steps.md` defines every step, its per-mode variations, and the per-mode flow summaries. Each step reports: `✓ Step [N]: [Brief status] - [Key metrics]`.

Track work in the **project's own tracker**, whatever the target repo's agent instructions name (in this fork's repos: beads, via `bd`). The project's tracking rules always win. Only where the project names none, use the harness Task tools (`TaskCreate`/`TaskUpdate`/`TaskGet`/`TaskList`; `TodoWrite` where those are unavailable). Wherever a step below says "tracker", read it through this rule.

## Delegation

Each phase runs in the agent or skill built for it, never improvised inline:

| Phase | Runs in | When |
|-------|---------|------|
| Research | `Explore` agents, in parallel | Optional in fast/code |
| Plan | `Plan` agent; plan files per the `project-organization` skill | Optional in code |
| Implement | Main loop; parallel `general-purpose` agents with file ownership in parallel mode | Every run |
| UI work | House design skills: `compose-multiplatform-ui`, `html-design-to-compose` | If Compose UI work |
| Simplify | `simplify` skill | When the diff breaches thresholds |
| Test | `do-test` skill | Every run except no-test |
| Review | `code-review` skill | Every run |
| Finalize | Inline: sync-back, docs, commit offer, journal | Every run |

Completion criterion: a run that reaches Finalize without `do-test` (unless no-test) and `code-review` having fired is incomplete.

Finalize always ends with a full-plan sync-back (every phase file, not just the current one), a docs check, a commit offer, and a journal entry in `docs/journals/`; details are in `references/workflow-steps.md` Step 6.

## References

- `references/intent-detection.md`: detection rules and routing logic
- `references/workflow-steps.md`: step definitions for all modes, including agent prompts
- `references/review-cycle.md`: interactive and auto review processes

## Workflow Position

**Typically follows:** a written plan or an agreed brainstorm
**Runs internally:** `project-organization` (paths/naming), `do-test` (Step 4), `code-review` (Step 5), `simplify` (Step 3.S)
