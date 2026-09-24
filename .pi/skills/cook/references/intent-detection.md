# Intent Detection Logic

Detect user intent from natural language and route to the appropriate workflow.

## Matching rules (apply to every check below)

- **Tokens, never substrings.** Split the input on whitespace, lowercase it, and match whole tokens or exact token sequences. `autocomplete` does not match `auto`; `quicksort` does not match `quick`; `breakfast` does not match `fast`.
- **Flags are trailing only.** A `--flag` counts as a mode request only when it appears after the task text (in the trailing tokens, per the usage pattern `/cook <task> --flag`). A `--flag` embedded inside the task sentence is part of the task, not a mode request: in `/cook add an --auto flag to the CLI --fast`, the trailing `--fast` is a flag; the embedded `--auto` is the feature being built.
- **Flag-as-subject guard.** A trailing flag whose immediately preceding tokens name it (`flag`, `option`, `argument`, `parameter`, `called`, `named`) is the task's subject, not a mode request: `/cook add a CLI flag called --auto` → interactive.
- **Negation cancels.** A keyword or phrase whose immediately preceding token is a negator (`no`, `not`, `don't`, `never`, `without`) is ignored: in "do not skip the tests", `not` precedes the matched phrase, so nothing is selected. Phrases are matched before the negator check, so `without` is consumed as part of the phrase `without tests` (a no-test request) and acts as a negator only before some other keyword.
- **Auto has no single-word keyword.** The only mode that removes every approval gate is entered deliberately or not at all: trailing `--auto`, or the exact phrase `trust me`. No bare word enables it.

## Detection Algorithm

```
FUNCTION detectMode(input):
  tokens = lowercase(whitespace-split(input))

  # Priority 1: explicit trailing flags (override all)
  IF trailing "--interactive": RETURN "interactive"
  IF trailing "--fast":        RETURN "fast"
  IF trailing "--parallel":    RETURN "parallel"
  IF trailing "--auto":        RETURN "auto"
  IF trailing "--no-test":     RETURN "no-test"
  # "--tdd" is composable and does not change mode selection

  # Priority 2: plan path detection
  IF input matches path pattern (./plans/*, plan.md, phase-*.md):
    RETURN "code"

  # Priority 3: keywords — whole tokens / exact phrases, negation-checked
  IF phrase "trust me":                             RETURN "auto"
  IF token in ["fast", "quick", "quickly", "asap"]: RETURN "fast"
  IF phrase in ["no test", "no tests", "skip test", "skip tests",
                "skip the test", "skip the tests", "without test", "without tests"]:
    RETURN "no-test"

  # Priority 4: complexity detection
  features = extractFeatures(input)  # comma-separated or "and"-joined items
  IF count(features) >= 3 OR token "parallel":
    RETURN "parallel"

  # Default: interactive workflow
  RETURN "interactive"
```

## Feature Extraction

Detect multiple features from natural language:

```
"implement auth, payments, and notifications" → ["auth", "payments", "notifications"]
"add login + signup + password reset"        → ["login", "signup", "password reset"]
"create dashboard with charts and tables"    → single feature (dashboard)
```

**Parallel trigger:** 3+ distinct features = parallel mode

Mode behavior (research/test/gates/progression) lives in the SKILL.md mode table. When multiple signals are detected, the algorithm's priority order above resolves the conflict: trailing flags > plan path > keywords > feature count > default.

## Examples

```
"/cook plans/260120-auth/phase-02-api.md"
→ Mode: code (path detected)

"/cook quick fix for the login bug"
→ Mode: fast ("quick" is a whole token)

"/cook add autocomplete to the search box"
→ Mode: interactive ("autocomplete" is not the token "auto" — and auto has no bare keyword anyway)

"/cook add an --auto flag to the CLI"
→ Mode: interactive (embedded flag is the task's subject, not trailing)

"/cook add a CLI flag called --auto"
→ Mode: interactive (trailing, but the flag-as-subject guard fires on "called")

"/cook do not skip the tests when refactoring"
→ Mode: interactive (matched phrase "skip the tests" is negated by "not")

"/cook implement auth, payments, notifications, shipping"
→ Mode: parallel (4 features)

"/cook refactor auth middleware --tdd"
→ Mode: interactive (default), with tests-first implementation behavior

"/cook implement dashboard trust me"
→ Mode: auto (exact phrase — the only keyword route to no gate stops)
```
