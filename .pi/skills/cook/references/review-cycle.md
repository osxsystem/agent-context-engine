# Code Review Cycle

Interactive review-fix cycle around the `code-review` skill, which returns findings by severity: critical / warning / suggestion.

## Interactive Cycle (max 3 cycles)

```
cycle = 0
LOOP:
  1. Invoke code-review skill → critical[], warnings[], suggestions[]

  2. DISPLAY FINDINGS:
     ┌─────────────────────────────────────────┐
     │ Code Review: [C] critical, [W] warnings │
     ├─────────────────────────────────────────┤
     │ Summary: [what implemented], tests      │
     │ [X/X passed]                            │
     ├─────────────────────────────────────────┤
     │ Critical ([C]): MUST FIX                │
     │  - [issue] at [file:line]               │
     │ Warnings ([W]): SHOULD FIX              │
     │  - [issue] at [file:line]               │
     │ Suggestions ([S]): NICE TO HAVE         │
     │  - [suggestion]                         │
     └─────────────────────────────────────────┘

  3. AskUserQuestion (header: "Review & Approve"):
     IF critical > 0:
       - "Fix critical issues" → fix, re-run do-test, cycle++, LOOP
       - "Fix all issues" → fix all, re-run do-test, cycle++, LOOP
       - "Approve anyway" → PROCEED
       - "Abort" → stop
     ELSE:
       - "Approve" → PROCEED
       - "Fix warnings/suggestions" → fix, cycle++, LOOP
       - "Abort" → stop

  4. IF cycle >= 3 AND user selects fix:
     → "⚠ 3 review cycles completed. Final decision required."
     → AskUserQuestion: "Approve with noted issues" / "Abort workflow"
```

## Auto-Handling Cycle (auto mode)

```
cycle = 0
LOOP:
  1. Invoke code-review skill → critical[], warnings[]

  2. IF critical == 0:
     → Auto-approve (warnings logged), PROCEED

  3. ELSE IF cycle < 3:
     → Auto-fix critical issues
     → Re-run do-test
     → cycle++, LOOP

  4. ELSE:
     → ESCALATE TO USER
```

## Severity Definitions

- **Critical:** security vulnerabilities (XSS, injection, OWASP), correctness bugs, broken contracts. Blocks approval.
- **Warning:** performance bottlenecks, architecture/pattern violations, coupling. Fix expected, but does not block auto-approve.
- **Suggestion:** simplifications, YAGNI/KISS/DRY improvements. Optional.

## Output Formats

- Waiting: `⏸ Step 5: Reviewed - [C critical / W warnings] - WAITING for approval`
- After fix: `✓ Step 5: Fixed [N] issues → 0 critical - Approved`
- Auto-approved: `✓ Step 5: Reviewed - 0 critical - Auto-approved`
- Approved: `✓ Step 5: Reviewed - [C critical / W warnings] - User approved`
