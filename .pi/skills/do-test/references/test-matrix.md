# Test Matrix

The row is the unit of work: every row leaves this phase with a verdict in the report.

`scenario | input | expected | surfaces | source`

Worked example, a fix for "discount code applies twice when Apply is double-clicked":

| # | Scenario | Input | Expected | Surfaces | Source |
|---|----------|-------|----------|----------|--------|
| 1 | Repro, pre-fix | two POST /cart/discount, code SAVE10 | **red**: total discounted twice | response, cart row | manual (stash) |
| 2 | Repro, post-fix | same | discount once, second call 409 | response, cart row, log | to write |
| 3 | Single apply still works | one POST, SAVE10 | 200, total −10% | response, cart row | `cart.discount.test.ts` |
| 4 | Two distinct codes | SAVE10 then FREESHIP | both applied, order-independent | cart row | to write |
| 5 | Expired code | EXPIRED2019 | 422, cart unchanged | response, cart row | existing |
| 6 | Concurrent apply | two parallel POSTs | one 200, one 409, one discount row | cart row, log | to write |

## Case classes

Take a row from each class the behaviour reaches:

- **Happy path**: the behaviour exactly as the oracle states it.
- **Boundary**: the values where logic changes: 0, 1, n−1, n, n+1, empty, single, max, over max, negative, absent/null, duplicate, very long, unicode/emoji, timezone edge, month and year rollover.
- **Error & invalid**: every error branch visible in the diff, plus the input that reaches it: wrong type, malformed, unauthorized, missing dependency, downstream timeout or failure.
- **State & ordering**: retry, double-submit, out-of-order arrival, partial failure mid-transaction, concurrent writers, stale cache.
- **Permission**: each role that may reach the behaviour, and each role that may not.
- **Seam**: the contract at every boundary the change crosses: serialized payload, DB schema, public API, platform bridge.

## Blast radius

How far the change reaches, and so what joins the matrix beyond the change itself:

1. **Callers**: every caller of each changed symbol (`codebase-retrieval`, then `grep` the symbol).
2. **Shared state**: tables, caches, globals, and files the change reads or writes, plus everything else that touches them.
3. **Contracts**: serialized formats, API responses, DB schema, public types consumed by another module or platform.
4. **Existing tests**: the tests already covering those areas; they run in Phase 3 whether or not they look related.
5. **Config & environment**: flags, env vars, and build variants whose value changes the path taken.

Size follows reach. One caller and no shared state earns a small matrix; an altered contract or shared table earns rows for every consumer.

## Coverage

The matrix is the coverage bar. Line coverage is the secondary check that finds what the matrix missed.

- The repo's own configured threshold wins wherever one exists.
- Otherwise: 80% lines, 70% branches, measured on the changed files rather than the whole project.
- Read the uncovered lines in changed files one by one: each is either a matrix row you missed or dead code worth reporting.
- Critical paths, meaning auth, payment, data mutation, and migrations, earn a row per branch whatever the percentage says.
