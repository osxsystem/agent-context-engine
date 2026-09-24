---
name: project-organization
description: Organize files, directories, and content structure in any project. Use when creating files, determining output paths, organizing existing assets, or standardizing project layout.
category: utilities
keywords: [files, directories, structure, layout]
argument-hint: "[directories or files to organize]"
metadata:
  author: osxsystem
  version: "1.1.0"
---

# Project Organization

Single source of truth for file locations, naming conventions, directory structures, and markdown content templates. Other skills invoke it to resolve any output path (plans, journals, reports, tests, docs, assets).

## Modes

| Mode | Trigger | Behavior |
|------|---------|----------|
| **Advisory** | Other skills/agents reference this skill | Return correct path + naming for requested file type |
| **Organize** | User invokes directly with dirs/files | Scan → propose changes → execute after confirm |

## Rule 1: Directory Categories

To place a file, walk this table top to bottom and take the **first** matching row:

| Category | Path | Purpose |
|----------|------|---------|
| Source code | `src/` or project root | Application code (follow language conventions, not managed here) |
| Tests | `tests/` or `test/` | Test suites (unit, integration, e2e), mirroring source structure |
| Plans | `plans/` | Implementation plans (`plans/{YYMMDD-HHmm}-{slug}/`), agent reports (`plans/{YYMMDD-HHmm}-{slug}/reports/` when plan-scoped, `plans/reports/` standalone), research |
| Documentation | `docs/` | Evergreen docs (`docs/{slug}.md`), journals (`docs/journals/`), ADRs (`docs/decisions/`) |
| Assets | `assets/{type}/` | Media, branding, designs, generated content |
| Scripts | `scripts/` | Build, deploy, utility scripts |
| Config | Root or `.config/` | Dotfiles, config files, env files (follow ecosystem conventions) |
| Guides | `guide/` or `guides/` | User-facing reference docs, tutorials |

**No row matches** (bug reports, scratch notes, sample data, anything else): leave the file where it is and list it under an **Unmatched** heading in the migration table with a suggested home, for the user to decide. Never invent a new top-level directory unprompted.

Full per-category trees and rules: `references/directory-patterns.md`.

## Rule 2: Naming Patterns

All filenames use **kebab-case**, self-documenting names. Three naming modes based on content temporality:

| Mode | Pattern | When to use | Examples |
|------|---------|-------------|---------|
| **Timestamped** | `{YYMMDD-HHmm}-{slug}` | Time-sensitive: plans, reports, journals, sessions | `260304-1530-auth-plan` |
| **Evergreen** | `{slug}` | Stable docs, configs, guides | `system-architecture.md` |
| **Variant** | `{slug}-{variant}.{ext}` | Multiple versions of same asset | `logo-dark.svg`, `hero-1920x1080.png` |

Slugs: lowercase, hyphens only, max 50 chars, readable without opening the file. Date format: `YYMMDD-HHmm` (`date +%y%m%d-%H%M`). Code files defer to language convention (kebab-case JS/TS/Python/Shell, PascalCase C#/Java/Kotlin/Swift, snake_case Go/Rust).

Slug generation, date formats, variant and report naming in full: `references/naming-conventions.md`.

## Rule 3: Nesting Logic

Decide between flat file vs folder based on output count:

| Scenario | Pattern | Example |
|----------|---------|---------|
| Single file output | Flat file in category dir | `docs/journals/260304-session-review.md` |
| Multi-file output | Self-contained subdirectory | `plans/260304-1530-auth-impl/plan.md` + `phase-01-*.md` |
| Scoped to parent | Nested under parent context | `plans/260304-1530-auth-impl/reports/scout-260304-1710-auth-module.md` |
| Platform-specific | Platform subdirectory | `assets/posts/twitter/`, `assets/posts/linkedin/` |
| Variant-based | Flat with variant suffix | `assets/branding/logo-light.svg`, `logo-dark.svg` |

**Empty directories:** Add `.gitkeep` to preserve in git.

## Rule 4: Markdown Body Standards

Universal rules for all markdown:
- Start with a `# Title` (H1)
- Use frontmatter (`---`) for metadata when the file is consumed by tools
- Keep sections ordered: context → content → next steps
- Use tables for structured data, lists for sequences
- Sacrifice grammar for concision

Required sections by type:

| Type | Key sections |
|------|-------------|
| **Plan** | frontmatter → overview → phases with status → dependencies → success criteria |
| **Phase** | context links → overview → requirements → architecture → impl steps → todo checklist → risks |
| **Report** | frontmatter → summary → findings → recommendations → unresolved questions |
| **Journal** | frontmatter → context → what happened → reflection → decisions → next |
| **Doc** | title → overview → content sections → references |
| **ADR** | status → context → decision → consequences → alternatives considered |
| **Changelog** | version blocks → categories (added/changed/fixed/removed/deprecated) |
| **README** | name → badges → description → quick start → usage → contributing → license |
| **Guide** | title → prerequisites → step-by-step → troubleshooting → FAQ |
| **Spec** | overview → requirements → constraints → API/interface → acceptance criteria |

Full templates: `references/markdown-body-templates.md`.

## Organize Mode Actions

When invoked directly with `/project-organization [targets]`:

1. **Scan**: List all files in target dirs, categorize by type
2. **Analyze**: Check naming violations, misplaced files, inconsistencies
3. **Propose**: Present a migration plan (from → to) as a table
4. **Confirm**: Ask user approval before any moves
5. **Execute**: Move/rename files, create missing directories
6. **Verify**: List final structure, flag any remaining issues

Completion criterion: every file in the targets is confirmed in place, in the migration table, or listed under Unmatched, with none unaccounted for.

**Safety:**
- Never overwrite existing files (prompt on conflict)
- Never touch `.git/`, `node_modules/`, `.env` files
- Respect `.gitignore` patterns

## Pre-Output Checklist

Before writing any file:
1. Determine category → get base path (Rule 1, first matching row)
2. Choose naming mode → timestamped/evergreen/variant (Rule 2)
3. Decide nesting → flat or subdirectory (Rule 3)
4. Apply body template if markdown (Rule 4)
5. Check if file/folder exists (avoid overwrite)
6. Create directory structure if needed
