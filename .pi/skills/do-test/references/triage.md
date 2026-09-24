# Triage

A BLOCKED verdict names a cause you confirmed. Everything here turns a red run into that cause.

## Classify first

Work down the ladder, stop at the first that holds, and record which one:

1. **Environment**: deps installed, services up, migrations applied, env vars set, right branch and build. Green before the change and red after rules this out.
2. **Test**: the expectation contradicts the oracle, the fixture is stale, the mock drifted from the real interface, or the assertion still encodes the old behaviour. The product is right and the test is wrong: correct the test and say so in the report.
3. **Data**: seed or fixture state rather than logic. Reproduces only with that data.
4. **Non-determinism**: passes and fails across identical runs. Run it 10×, then vary order and seed. A flake is a defect: report it with the shared state or timing you suspect, and the row stays UNVERIFIED until it holds.
5. **Product code**: the change is wrong. The default once 1-4 are excluded.

## Confirm the cause

A hypothesis becomes a cause when you can make the failure appear and disappear by changing one thing: the input, the line, or the config. Until it flips on demand, it reports as a hypothesis.

Cut to the smallest reproduction that still fails: fewest steps, smallest input, least setup. That reproduction goes verbatim into the report.

When two hypotheses fall in a row, stop guessing: go back to Ground, re-read the evidence end to end, and binary-search the failure by halving the input, the diff, or the steps until the smallest failing half names the cause.

## Audit the greens

A pass is evidence only where the test ran and asserted:

- Test count matches expectation, because a renamed or misplaced file makes a suite pass by running nothing.
- Every skip is accounted for and intentional.
- Filters (`-t`, `--only`, `.only`, tags, Gradle task scope) ran the tests you meant.
- Where a pass looks too easy, break the behaviour in the product code, watch the test go red, then restore. A test that stays green through a broken behaviour asserts nothing.

## What a failure carries into the report

- **Repro**: exact steps and commands, from a clean state.
- **Expected vs observed**: quoted from output, not paraphrased.
- **Evidence**: command, output excerpt, surface readings.
- **Class**: from the ladder above.
- **Cause**: the confirmed one, with what made it flip.
- **Fix**: the recommendation, and its own blast radius.
