---
name: use-git-worktree
description: Create an isolated git worktree under `.worktrees/` before starting work, via `git worktree add` on a `feat/` or `fix/` branch. Use when beginning a feature, a bug fix, a refactor, a dependency upgrade, or an implementation plan that will touch more than one file, even when the user never says "worktree". Skip it for one-line and single-file edits, and stay in place when the user says to work on the current branch. Also use when unsure whether the session is already inside a worktree.
---

# Use Git Worktree

Every feature and every bug fix starts in its own worktree under `.worktrees/`, so the main checkout keeps its branch and its uncommitted state while the work happens somewhere else.

Worth knowing before step 1: this is not a clone. A worktree shares one object store, one set of refs, and one config with the main checkout, and owns only its own `HEAD`, index, and files. That is why it costs almost nothing, and why a branch made here is visible everywhere at once.

## 1. Are you already isolated?

One call answers it, and gives you the repo root for the steps below:

```bash
git rev-parse --path-format=absolute --git-dir --git-common-dir --abbrev-ref HEAD --show-toplevel
```

**Lines 1 and 2 differing means you are in a linked worktree.** Stop, report the path and branch, and do not nest a second one. Equal means a plain checkout, so carry on. Line 3 is the branch, line 4 is the repo root.

`--path-format=absolute` is the load-bearing flag. Without it, git answers the second question relative to your cwd, so from a subdirectory you get `/repo/.git` against `../../.git`: the same directory, spelled two ways, which a string comparison calls a worktree. The skill would then report "already isolated" from an ordinary subdirectory and never create anything.

Two cases this deliberately does not special-case. A submodule reports both paths as its own `.git/modules/<name>`, equal, so it reads as a plain checkout, which is what you want. A worktree *of* a submodule reports them differently, so it reads as a worktree, which is also what you want. Do not add a submodule guard here: the two paths are identical inside a submodule, so any guard built on them differing is dead code.

## 2. Name the branch

`feat/<slug>` for a feature, `fix/<slug>` for a bug fix, kebab-case slug from the task: `feat/oauth-login`, `fix/crash-on-rotate`. Anything that is not a bug fix takes `feat/`, refactors and upgrades included. Two prefixes is a deliberate choice, so do not invent a third. The worktree path mirrors the branch, so `.worktrees/feat/oauth-login`.

## 3. Make `.worktrees/` safe to create

Run this and step 4 from the repo root (line 4 above), or a monorepo subdirectory gets its own stray `.worktrees/`:

```bash
git check-ignore -q .worktrees/ || printf '\n/.worktrees/\n' >> .gitignore
```

Do this before creating, never after. Until the directory is ignored, `git status` offers `?? .worktrees/` and `git add -A` stages it as an embedded repository: a single gitlink entry at mode `160000`, not a copy of the files. So the damage is not bulk, it is a commit that points at a repository nobody else can fetch, and clones of the outer repo silently lack the contents.

Two bits of that line read like noise and are not:

- **Trailing slash on the query.** A directory-only pattern cannot match a path git has not seen on disk, so `check-ignore -q .worktrees` reports "not ignored" on a fresh repo even when the rule is already there, and appends it again on every run. `.worktrees/` matches whether or not the directory exists.
- **Leading newline in the `printf`.** A `.gitignore` with no trailing newline would otherwise absorb the rule into its last line, turning `node_modules` into `node_modules.worktrees/`.

The written rule is anchored (`/.worktrees/`) while the query is not, and that asymmetry is intentional: anchoring the rule keeps it to the repo root, and an unanchored query still matches there whichever form an existing rule uses.

Ignoring the directory settles git, but not everything else that reads the tree. A worktree is a full second checkout, so tooling that walks files rather than asking git (linters, license scanners, custom CI guards, Docker build contexts, IDE and Gradle file watchers) sees every file twice and reports the copy against the original. If a check starts failing on paths under `.worktrees/`, the fix belongs in that tool's skip list, next to wherever it already skips `node_modules`. A `.dockerignore` needs the rule separately; it does not read `.gitignore`.

## 4. Create it and move in

```bash
git worktree add .worktrees/feat/oauth-login -b feat/oauth-login origin/main
```

Name the base ref explicitly. Omit it and git branches from the main checkout's current `HEAD`, which this skill's whole premise says you left parked wherever the user had it, so the new branch quietly forks from last week's feature work. Use the default branch tip, `origin/main` or whatever this repo calls it, and fall back to the local default branch when there is no remote. Report whichever you used.

Parent directories are created for you, so there is no `mkdir` step. Then move the session in, so later tool calls read and write the worktree rather than the main checkout:

- If you have an `EnterWorktree` tool, call it with `path: ".worktrees/feat/oauth-login"`. Entering a path outside `.claude/worktrees/` works on first entry from the launch directory, which is the normal case; a session already inside a worktree, or an agent with a pinned cwd, may be restricted to `.claude/worktrees/` and refuse. It will not delete a worktree it did not create, so leave with `action: "keep"`.
- Otherwise `cd` in.

**Branch already exists:** drop `-b` and the base ref to check it out instead, `git worktree add .worktrees/feat/oauth-login feat/oauth-login`. If `git worktree list` already shows a worktree on that branch, enter that one rather than making a rival. Git refuses to check out one branch in two worktrees, so this is a real dead end, not a warning.

**Creation refused by the sandbox:** say so plainly, then work in the main checkout. Quietly proceeding in place is the failure worth avoiding, because the user carries on believing their branch is protected when it is not.

**Repo has no commits yet:** there is no `HEAD` to branch from and `worktree add` fails. Say so and work in place; the first commit has to land before isolation is possible.

## Report

```
Worktree  .worktrees/feat/oauth-login
Branch    feat/oauth-login (new, from origin/main)
Ready to implement <task>
```

Setting up the workspace is the whole job. Installing dependencies and running the suite belong to whatever comes next, so hand back here rather than spending a full install and test run the user did not ask for.
