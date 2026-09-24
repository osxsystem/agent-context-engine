# Workflow Steps

All modes share core steps with mode-specific variations. "Tracker" below means the project's own task tracker per the rule in SKILL.md (project tracker first, e.g. `bd` in beads repos; harness Task tools only where the project names none); every step remains functional whichever tracker is active.

## Step 0: Intent Detection & Setup

1. Parse input with `intent-detection.md` rules
2. Log detected mode: `✓ Step 0: Mode [X] - [reason]`
3. If mode=code: detect plan path, set active plan
4. Create workflow step items in the tracker (with dependencies if complex)

**Output:** `✓ Step 0: Mode [interactive|auto|fast|parallel|no-test|code] - [detection reason]`

## Step 1: Research (skip if fast/code mode)

- Spawn parallel `Explore` agents, one per question: codebase structure, existing patterns, external constraints:
  `Agent(subagent_type="Explore", prompt="Find and summarize [topic]. Report ≤150 lines with file references.")`
- Parallel mode: optional, max 2 explorers if complex
- Save reports to the plan's `research/` folder, with path and naming from the `project-organization` skill

**Output:** `✓ Step 1: Research complete - [N] reports gathered`

### [Review Gate 1] Post-Research (skip if auto mode)
- Present research summary to user
- `AskUserQuestion`: "Proceed to planning?" / "Request more research" / "Abort"

## Step 2: Planning

- Spawn a `Plan` agent with the research reports as context
- Write `plan.md` + `phase-{NN}-{name}.md` under `plans/{date-slug}/`, with paths, naming, and body templates from the `project-organization` skill
- **Fast:** minimal plan from a single quick `Explore` pass, focused on action
- **Parallel:** plan must include a dependency graph and a file ownership matrix
- **Code:** skip, because a plan exists; parse its phases

**Output:** `✓ Step 2: Plan created - [N] phases`

### [Review Gate 2] Post-Plan (skip if auto mode)
- Present plan overview with phases
- `AskUserQuestion`: "Approve" (start implementation) / "Revise" (apply feedback, re-present) / "Abort"

## Step 3: Implementation

**Task hydration:**
1. List the tracker first, checking for existing tasks (hydrated by a planning session)
2. If tasks exist → pick them up, skip re-creation
3. If no tasks → read plan phases, create a tracker item for each unchecked `[ ]` item with priority order and metadata (`phase`, `planDir`, `phaseFile`)
4. Declare blocking dependencies between items where the tracker supports them

### Conformance Checklist (before writing code)

Before implementing each phase:

1. **Read `./docs/code-standards.md`** (if present) and confirm naming, file structure, and error-handling patterns still match the repo.
2. **Scout adjacent code patterns** in the files being modified and follow the same import, logging, and error-wrapping style.
3. **Check for existing helpers** before creating new utilities so the change stays DRY.
4. **Verify interface contracts** so new code extends the current surface instead of creating a parallel one.
5. **Cross-check the plan checklist** so every file in the phase inventory is actually addressed.

After each file is modified:
- **Compile check:** run the relevant project compile/type-check command
- **Pattern verify:** confirm the new code matches adjacent conventions
- **Import check:** confirm no circular dependency or dead import was added

### `--tdd` Flag Behavior

When `--tdd` is active, Step 3 splits into sub-steps per phase:

```
Step 3.T: Write tests for CURRENT behavior (regression safety net)
Step 3.I: Implement changes (refactor, new code)
Step 3.V: Verify all tests from 3.T still pass + compile gates
```

Tests from Step 3.T document the current behavior. If any fail after Step 3.I, the refactor broke something and must be fixed before the workflow proceeds.

**All modes:**
- Mark tracker items in-progress when picked up and complete immediately when done
- Execute phase tasks sequentially (Step 3.1, 3.2, etc.)
- UI work follows the house design skills: `compose-multiplatform-ui`, and `html-design-to-compose` when implementing from a design spec
- Run type checking after each file

**Parallel mode:**
- Launch multiple `general-purpose` agents, one per parallel group, each prompt carrying its file ownership boundaries from the plan's ownership matrix
- Assign each tracker item to its agent and mark it in-progress at launch
- Wait for the whole parallel group before starting the next

**Output:** `✓ Step 3: Implemented [N] files - [X/Y] tasks complete`

### Step 3.S: Conditional Simplify (live-diff gated)

Recompute signals from the live worktree, both tracked changes **and** untracked files, so new files count:

```bash
tracked=$(git diff --numstat HEAD --ignore-all-space)
untracked=$(git ls-files --others --exclude-standard | while IFS= read -r f; do
  printf '%s\t0\t%s\n' "$(wc -l < "$f" | tr -d ' ')" "$f"
done)
totals=$(printf '%s\n%s\n' "$tracked" "$untracked")
loc=$(echo "$totals" | awk '{s+=$1+$2} END {print s+0}')
files=$(echo "$totals" | awk 'NF{c++} END {print c+0}')
maxFile=$(echo "$totals" | awk 'BEGIN{m=0} {if ($1+0>m) m=$1+0} END {print m+0}')
```

Thresholds: 400 total LOC / 8 files / 200 LOC in a single file. If any is breached, invoke the `simplify` skill scoped to the union of `git diff --name-only HEAD` and `git ls-files --others --exclude-standard`.

After it returns, log only, never re-run or block:
- `git diff --shortstat HEAD -- [file-list]` changed → "simplify made scoped edits"
- unchanged → "simplify ran clean"

**Output:** `✓ Step 3.S: Simplify [ran|skipped] - [scoped changes|clean|under threshold]`

### [Review Gate 3] Post-Implementation (skip if auto mode)
- Present implementation summary (files changed, key changes)
- `AskUserQuestion`: "Proceed to testing?" / "Request implementation changes" / "Abort"

## Step 4: Testing (skip if no-test mode)

- Write tests: happy path, edge cases, errors
- Invoke the `do-test` skill with scope = the files/features changed this phase
- 100% pass required. On failures: root-cause fix, then re-invoke `do-test`, repeating until green
- **Forbidden:** fake mocks, commented-out tests, loosened assertions

**Output:** `✓ Step 4: Tests [X/X passed] - do-test invoked`

### [Review Gate 4] Post-Testing (skip if auto mode)
- Present test results summary
- `AskUserQuestion`: "Proceed to code review?" / "Request test fixes" / "Abort"

## Step 5: Code Review

- Invoke the `code-review` skill on the changes since the phase started → findings by severity (critical / warning / suggestion)
- **Interactive/Parallel/Code/No-test:** interactive cycle (max 3) per `review-cycle.md`; requires user approval
- **Auto:** auto-approve at zero critical findings; auto-fix criticals (max 3 cycles), then escalate to user
- **Fast:** single pass, no fix loop; user approves or aborts

**Output:** `✓ Step 5: Review [C critical / W warnings] - [Approved|Auto-approved] - code-review invoked`

## Step 6: Finalize (inline, every run, never skipped)

1. **Sync-back:** sweep all `phase-XX-*.md` files in the plan directory; mark every completed item `[ ] → [x]` based on completed tasks (including earlier phases); update `plan.md` status/progress (`pending`/`in-progress`/`completed`) from actual checkbox state, changing only the Status cells, preserve table structure. Note any completed task that cannot be matched to a phase file.
2. **Docs:** update `./docs` if the changes warrant it.
3. Mark the tracker items complete after sync-back verification.
4. **Onboarding check:** new API keys, env vars, setup steps the user must know about.
5. **Commit offer:** ask the user, then stage and commit with a conventional commit message.
6. **Journal:** write a concise technical journal entry at `docs/journals/{YYMMDD-HHmm}-{slug}.md` using the `project-organization` journal template.

Completion criterion: all phase files swept, plan.md status matches checkbox reality, journal written.

**Auto mode:** continue to the next phase automatically, from Step 3.
**Others:** ask user before the next phase.

**Output:** `✓ Step 6: Finalized - sync-back complete - journal written - [committed|commit declined]`

## Mode-Specific Flow Summary

Legend: `[R]` = Review Gate (human approval required)

```
interactive: 0 → 1 → [R] → 2 → [R] → 3 → [R] → 4 → [R] → 5(user) → 6
auto:        0 → 1 → 2 → 3 → 4 → 5(auto) → 6 → next phase (NO stops)
fast:        0 → skip → 2(fast) → [R] → 3 → [R] → 4 → [R] → 5(single pass) → 6
parallel:    0 → 1? → [R] → 2(parallel) → [R] → 3(multi-agent) → [R] → 4 → [R] → 5(user) → 6
no-test:     0 → 1 → [R] → 2 → [R] → 3 → [R] → skip → 5(user) → 6
code:        0 → skip → skip → 3 → [R] → 4 → [R] → 5(user) → 6
```

**Key difference:** `auto` mode is the ONLY mode that skips all review gates.

Delegation requirements, task-tracking rules, and the step output format are defined once in SKILL.md (Delegation and Pipeline sections); a step is only skipped when its mode row says so.
