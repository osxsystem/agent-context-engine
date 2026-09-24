# Cook Skill

End-to-end feature implementation pipeline with intent detection. Modes, gates, and workflow are defined in [SKILL.md](./SKILL.md); this file is only the human-facing quick start.

## Installation

Copy the `cook/` folder to your Claude skills directory:
```bash
cp -r cook ~/.claude/skills/
```

## Usage

```bash
/cook <natural language task OR plan path>
```

The skill detects your intent (flags, plan paths, keywords, feature count) and routes to the matching mode.

## Examples

```bash
# Interactive mode (default)
/cook implement user authentication

# Execute existing plan
/cook plans/260120-auth

# Fast mode (skip research)
/cook quick fix for login bug

# Auto mode (no gate stops)
/cook implement dashboard trust me

# Parallel mode (multi-agent)
/cook implement auth, payments, notifications

# TDD flag (composable with any mode)
/cook refactor auth middleware --tdd
```

## Version

1.0.0 - initial osxsystem release: pipeline built on this fork's skills (`do-test`, `code-review`, `simplify`, `project-organization`, house design skills) and native agent types (`Explore`, `Plan`, `general-purpose`)
