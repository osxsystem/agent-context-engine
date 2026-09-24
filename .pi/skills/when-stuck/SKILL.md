---
name: when-stuck
description: Techniques for design and architecture stuck-ness. Use when a design keeps sprouting special cases, when every option feels forced, or when the same problem recurs in different places.
---

# When stuck

Five ways to re-frame a design problem that will not resolve. Each one is a move, not a mood: pick by symptom, make the move, see what it exposes.

**This is for design and architecture stuck-ness only.** Two neighbours own the other kinds, and reaching for this one instead wastes the session:

| What is stuck | Use |
|---|---|
| A bug you cannot reproduce or explain | the `/diagnosing-bugs` skill |
| A plan or decision you have not settled | the `/grilling` skill |
| The *shape* of a solution, when it won't resolve or every option feels wrong | this skill |

Once a move lands, the `/codebase-design` skill is the vocabulary for the shape it exposed, complementary to this skill rather than competing with it.

## Inversion

**Reach for it when** you catch yourself saying it has to be done this way, or the design feels forced but you cannot say why.

State the load-bearing assumption out loud, then negate it and design from the negation. You are not looking for the inverted design to win; you are looking for what the negation exposes about the original.

*Worked example.* "Every request has to hit the cache first." Inverted: nothing is cached. What breaks? If the honest answer is "one endpoint gets slow", the cache was never the architecture. It was one endpoint's optimisation, and modelling it as a layer was the mistake.

## The scale game

**Reach for it when** you cannot tell whether a design holds up, or "it depends" is the only answer you can give about load.

Run the design at three sizes: zero, one, and a million. Designs break at the extremes in ways the middle hides.

*Worked example.* A sync engine that reconciles on every change. At zero changes it does nothing, which is fine. At one, it is obviously correct. At a million it is a thundering herd, and you learn the design's real dependency is batching, which was nowhere in the diagram.

## Simplification cascade

**Reach for it when** the same idea is implemented several ways, special cases keep accreting, or every fix adds a branch.

Stop looking for the change that handles the next case. Look for the one change that **deletes components**, the reframing after which several of the special cases stop existing rather than getting handled.

*Worked example.* Four code paths for four auth providers, each with its own refresh quirk. The cascade is not a fifth path; it is noticing that three of the four differ only in token lifetime, which turns three paths into one path with a parameter.

## Meta-pattern

**Reach for it when** this feels like a problem you have solved before somewhere else, or the same shape keeps recurring in unrelated parts of the system.

Find three instances and name what they share. Naming the shape is the work, because an unnamed recurring pattern gets re-solved from scratch every time it appears.

*Worked example.* Retry-with-backoff in the uploader, debounce in the search box, and rate-limiting on the API client are three faces of "an operation whose timing is governed by feedback from its own failures". Named, they can share one abstraction; unnamed, they stay three.

## Collision

**Reach for it when** every conventional option is inadequate and you need something you have not thought of yet.

Force two unrelated concepts together and take the result seriously for a few minutes before rejecting it. Most collisions produce nothing; the technique is cheap enough that this is fine.

*Worked example.* "What if the migration were a test?" is nonsense until you notice both are things you run once against a known starting state and assert an ending state, and now the migration has a dry-run mode it did not have before.

## Done when

You have a move to make, not a summary of your situation. If the session ends with a restatement of the problem, the technique did not fire, so pick a different one rather than describing the stuck-ness more carefully.

---

Derived from the problem-solving agent patterns in [Microsoft Amplifier](https://github.com/microsoft/amplifier) (MIT), commit `2adb63f858e7d760e188197c8e8d4c1ef721e2a6`.
