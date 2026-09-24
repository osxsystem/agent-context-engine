---
name: port-from-repo
description: Bring a capability across from another codebase, study it, argue against it, then adapt it to this codebase's idiom instead of transplanting it.
disable-model-invocation: true
---

# Port from repo

You saw something work in another codebase and you want it here. This skill brings the capability over without bringing its architecture with it.

**Adapt, don't transplant.** The code you are reading was shaped by constraints that are not yours: its dependency graph, its error convention, its platform assumptions, its idea of where a boundary goes. What travels is the *approach*; the expression gets rewritten in this codebase's idiom. A port that reads like a foreign body is a failed port, even when the tests are green.

Ask the user for the source, whether a GitHub URL, an `owner/repo`, or a local path, and for the capability they want, if they haven't already said.

## The source is read-only

Fetch it outside this repository: a shallow clone in a temp directory, or `gh` reads against the API when you only need a few files. Never add it as a submodule, never vendor the tree, never edit it. You are reading a primary source, not acquiring a dependency.

Prefer reading over cloning when the capability is small and the repo is large, because a clone you have to search is slower than the two files you actually need.

Read the licence before you read the code. Porting an approach rather than an expression is the safer footing, but a licence that restricts derivative work is a reason to stop and put it to the user, not a wall to work around.

## Understand

Before any judgement about whether to port, be able to answer four questions:

- **What does it do?** State the behaviour without referring to its code.
- **How does it earn its keep?** Name the hard part it solves. Most ported code is a little insight surrounded by a lot of plumbing, so find the insight.
- **What holds it up?** The dependencies, framework assumptions, and platform APIs it stands on, and which of those exist here.
- **What did it cost the source?** The constraints its own codebase accepted in order to have it.

Report the four answers back before going on. Not being able to answer the second one means you are ready to copy, not to port, and copying is the failure this skill exists to prevent.

## Challenge

Run the `/grilling` skill on one question: **should we bring this over at all?**

Don't skip this because the answer feels obvious; it is the step that separates a port from a copy. The frontier to push on:

- What is the smallest thing that solves our actual problem? It is usually a fraction of what the source built.
- What do we already have that overlaps? A port that duplicates an existing seam makes the codebase worse, not better.
- Which of the source's constraints are we inheriting, and do we accept them?
- What happens if we do nothing?

Grilling ends in decisions. Carry them into the next phase: what is in scope, what is cut, and what we are deliberately doing differently from the source.

## Adapt

Decide where it lands before writing anything. Use the `/codebase-design` skill for the vocabulary: the seam it sits behind, how deep the module is, what the interface exposes. Treat the source's boundaries as evidence, not instruction: it drew them for its own codebase.

Then build with the `/tdd` skill, one vertical slice at a time, at seams agreed with the user. Retyping rather than pasting is not ceremony; it is what surfaces the assumptions that don't hold here.

Translate as you go:

- **Naming** to this project's domain language, so read `CONTEXT.md` where it exists.
- **Error handling** to this codebase's convention, not the source's.
- **Dependencies** to what the manifest already has. A dependency the source has and this codebase doesn't is usually hiding a seam: the thing to port is what the source used it *for*, re-expressed with what's already here, whether a constructor parameter, an interface, or a plain function. Adding the dependency is the last resort, not the first, and a decision to surface to the user, not take.

### Porting into Kotlin Multiplatform

The common case in this repo is lifting something out of an Android-only or iOS-only sample into `commonMain`, which is where "adapt, don't transplant" bites hardest.

- Anything touching a platform API cannot cross as-is. The `expect`/`actual` versus interface-plus-DI call belongs to the `/kmp-module-setup` skill.
- Lifecycle assumptions do not travel. Code that assumes an Android `Activity` recreation, or a `UIViewController` appearing, has to be re-expressed as explicit state before it can live in shared code.
- A captured `Context` or `UIViewController` is a signal you are porting the plumbing rather than the insight. Find the behaviour underneath it.

## Verify

Close out with the `/code-review` skill.

Then record provenance in the commit message: the source repository, its licence, the specific commit or file path it came from, and what you changed on the way in. A port whose origin is folklore cannot be re-checked when the source fixes a bug.

## Done when

- The capability is reachable through an interface this codebase already uses.
- It has a test at a seam agreed with the user, and that test would pass against a fresh implementation of the same behaviour.
- Nothing in the diff reads as imported: naming, error handling, and structure match the code around it.
- The commit message names the source repository and the commit or path.

---

Derived from the `xia` skill in ClaudeKit, re-authored for this repo.
