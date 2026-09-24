---
name: to-prd
description: Turn the current conversation into a Product Requirements Document, synthesis first, questions only for what can't be inferred, saved in the repo and linked from the tracker.
disable-model-invocation: true
---

This skill takes the current conversation and codebase understanding and produces a Product Requirements Document (PRD). Synthesize first: fill every section you can from what has already been discussed. Interview only for the sections that are genuinely unanswerable from context, never re-asking what the conversation already settled.

A PRD sits **before** a spec. The spec (`to-spec`) says what to build and how to test it; the PRD says why it's worth building, for whom, and what success looks like. It's the document a CEO, BA, and engineer can all read and mean the same thing by, which is why it borrows the project's established vocabulary rather than inventing its own.

Its place in the flow: after the idea has been sharpened (`grill-with-docs`) and the vocabulary nailed (`domain-modeling`), before `to-spec` turns it into buildable work, in `grill-with-docs → domain-modeling → to-prd → to-spec → to-tickets → implement`.

The issue tracker should have been provided to you. If not, tell the user to run `/setup-osxsystem-skills`.

## Process

1. Ground yourself in the repo, if you haven't already. Use the project's domain glossary vocabulary (`CONTEXT.md`) throughout the PRD, and respect any ADRs touching this area, because a PRD that contradicts a recorded decision needs to say so explicitly, not silently.

2. Draft the full document from what you already know: the conversation, any research or grilling that preceded it, the codebase. Fill every section of the template below that you can defend. Where a section rests on something you believe but weren't told, write it anyway and flag it as an assumption; assumptions the team can shoot down are worth more than blanks.

   If drafting surfaces a term the PRD leans on that `CONTEXT.md` doesn't pin down, or one word doing two jobs, call the Skill tool with "domain-modeling" to sharpen it before that term anchors a section. A PRD is the first document a non-engineer reads; a fuzzy term here propagates into every spec and ticket downstream.

3. Identify the genuine gaps. Some sections can't be invented on the user's behalf, because they're decisions, not facts:

   - **Key Results**: the targets and their numbers are the user's call.
   - **Contacts**: who owns this, who must be consulted.
   - **Release cut**: what's in the first version versus later.

   Ask about these in **one batched round** of questions, and only for the ones the conversation didn't already answer. Facts are your job to find; decisions are the user's to make. If the user defers, keep the section with a `⚠ TBD: <who decides>` marker so the gap survives into the document instead of vanishing.

   If you find that *most* sections are gaps, the idea isn't ripe for a PRD, so say so and point the user at `/grill-with-docs` to sharpen it first rather than interviewing a PRD into existence.

4. Write the PRD using the template below. Write for a reader outside the building: short sentences, no jargon beyond the project glossary, every claim either grounded or flagged. Save it as `PRD-<product-name>.md` wherever the project keeps product documents; if there is no established place, use `docs/product/`.

5. Publish a tracker issue titled `PRD: <product name>` containing the Summary section and a link to the file. Do **not** apply the `ready-for-agent` label, because a PRD is not implementable work; it's the anchor that specs and tickets will reference.

6. Point the way forward: when the user is ready to build, the next step is `/to-spec`, and each spec that grows out of this PRD should reference it.

<prd-template>

## 1. Summary

2-3 sentences: what this document is about and why it exists.

## 2. Contacts

| Name | Role | Comment |
| ---- | ---- | ------- |

Who owns this, who must be consulted, who just needs to know.

## 3. Background

- What is this initiative about?
- Why now: has something changed? Did this recently become possible?

## 4. Objective

- What's the objective, and why does it matter?
- How does it benefit the company and the customers?
- How does it align with the vision and strategy?

**Key Results**: how success will be measured, in SMART format. Numbers and deadlines are decisions, not guesses; if the user hasn't made them, mark them `⚠ TBD`.

## 5. Market Segment(s)

For whom are we building this, and what constraints exist? Define segments by the problems and jobs people have, not by demographics.

## 6. Value Proposition(s)

- Which customer jobs and needs are we addressing?
- What will customers gain? Which pains will they avoid?
- Which problems do we solve better than the alternatives?

## 7. Solution

- **7.1 UX**: user flows, wireframes or prototype references if any exist.
- **7.2 Key Features**: what the product does, feature by feature.
- **7.3 Technology** *(optional)*: only where a technical choice shapes the product.
- **7.4 Assumptions**: what we believe but haven't proven. Every flagged assumption from the drafting step lands here so the team can validate or kill it.

## 8. Release

Relative timeframes, never exact dates. What goes in the first version; what waits. The first-version cut is a decision, so if it hasn't been made, mark it `⚠ TBD` rather than inventing it.

</prd-template>
