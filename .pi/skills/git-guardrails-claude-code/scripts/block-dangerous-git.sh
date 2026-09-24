#!/bin/bash

# PreToolUse hook: block dangerous git commands before Claude runs them.
# Exit 2 = blocked, exit 0 = allowed.
#
# Fails CLOSED: if the payload can't be read, the command is blocked rather
# than waved through. A guardrail that silently stops guarding is worse than
# no guardrail, because it still looks installed.

if ! command -v jq >/dev/null 2>&1; then
  echo "BLOCKED: git guardrail cannot run (jq not found on PATH). Install jq or remove the hook." >&2
  exit 2
fi

INPUT=$(cat)

if ! COMMAND=$(printf '%s' "$INPUT" | jq -re '.tool_input.command' 2>/dev/null); then
  echo "BLOCKED: git guardrail could not read .tool_input.command from the hook payload. Blocking rather than allowing an unchecked command." >&2
  exit 2
fi

# Patterns are anchored to the start of each command segment, so a dangerous
# invocation is caught wherever it runs, while a mere *mention* of one (in a
# commit message, an echo, a grep pattern) is not.
DANGEROUS_PATTERNS=(
  "git[[:space:]]+push"
  "git[[:space:]]+reset([[:space:]].*)?[[:space:]]--hard"
  "git[[:space:]]+clean([[:space:]].*)?[[:space:]]-[[:alpha:]]*f[[:alpha:]]*"
  "git[[:space:]]+branch([[:space:]].*)?[[:space:]]-D"
  "git[[:space:]]+checkout[[:space:]]+\."
  "git[[:space:]]+restore[[:space:]]+\."
)

# Split on shell separators so `cd foo && git push` is still caught, then strip
# leading env assignments (FOO=1 git push) and `git -C <path>` redirection.
SEGMENTS=$(printf '%s' "$COMMAND" | sed -E 's/(\&\&|\|\||;|\|)/\n/g')

while IFS= read -r segment; do
  segment="${segment#"${segment%%[![:space:]]*}"}"
  segment=$(printf '%s' "$segment" | sed -E 's/^([A-Za-z_][A-Za-z0-9_]*=[^[:space:]]*[[:space:]]+)+//')
  segment=$(printf '%s' "$segment" | sed -E 's/^git[[:space:]]+-C[[:space:]]+[^[:space:]]+[[:space:]]+/git /')
  [ -z "$segment" ] && continue

  for pattern in "${DANGEROUS_PATTERNS[@]}"; do
    if printf '%s' "$segment" | grep -qE "^${pattern}([[:space:]]|$)"; then
      echo "BLOCKED: '$COMMAND' matches dangerous pattern '$pattern'. The user has prevented you from doing this." >&2
      exit 2
    fi
  done
done <<< "$SEGMENTS"

exit 0
