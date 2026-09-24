# Report Format

Verdict first, evidence throughout. Prefer concision over grammar.

```markdown
# Test Report — {date} — {scope}

**Verdict: SHIP | BLOCKED | UNVERIFIED** — {one line: why}

**Under test:** {feature or fix} · {commit range / files} · oracle: {source}

## Matrix
| # | Scenario | Expected | Surfaces | Result | Evidence |
|---|----------|----------|----------|--------|----------|
| 1 | ... | ... | response, db | PASS | `npm test -- cart` 42/42; one discount row |
| 2 | ... | ... | response | FAIL | F1 |
| 3 | ... | ... | ui | UNVERIFIED | no staging env |

## Suites
| Suite | Command | Passed | Failed | Skipped | Duration |

## Failures
### F1 — {title}
- **Repro**: steps from a clean state
- **Expected / Observed**: ... / ... (quoted output)
- **Class**: environment | test | data | non-determinism | product code
- **Cause**: confirmed, plus what made it flip
- **Fix**: recommendation + its blast radius

## Unverified
| Row | Why | What would close it |

## Coverage
Changed files under threshold, each with the uncovered branch and whether a matrix row is missing.

## Preflight & build
typecheck / lint / build — status, new warnings.

## Recommendations
1. {critical|high|medium|low} — {action}

## Assumptions & open questions
- {behaviour with no oracle, and what you assumed}
```

## Rules

- Every FAIL gets its own `F` entry, however many there are.
- Evidence cites the command and the reading it produced, in place of a claim that something works.
- Screenshot paths inline, relative to the report.
- Recommendations ranked critical → low.
- Past ~200 lines: keep the matrix and failures whole, and move suite logs to an appendix file beside the report.
- Save with the `project-organization` timestamped report pattern.
- Coverage thresholds live in `test-matrix.md`.
