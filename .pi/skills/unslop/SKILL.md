---
name: unslop
description: "Strip AI tells from human-facing prose and give it a voice. Use before publishing or sending anything a person reads: a README, docs page, PR description, release notes, changelog, commit body, blog post, or email. Also use when text is called slop, AI-sounding, generic, corporate, or wordy, or when someone asks to make writing sound human. Skip it for chat replies, one-line commit subjects, and code comments written in passing. For a document an agent reads, run writing-for-agents first and this second."
---

# Unslop

Slop is writing that could have come from anywhere, about anything. The tells are symptoms. The condition is prose carrying no information and no person, and a draft can be free of every tell and still have it.

So this is two jobs. Cut the tells, then check that something is left: a claim, a number, a mechanism, an opinion. A sterile paragraph reads as machine-made just as loudly as a florid one, which is why deleting adjectives until the text is beige is a failure and not a fix.

## Process

1. **Read the whole thing first.** Slop is often structural: three sections restating one idea at three lengths, a summary of a page the reader just read. A sentence-level pass never sees that.
2. **Set the register.** It decides which rules apply. See [Register](#register).
3. **Run the checker.** `python3 scripts/check-tells.py PATH`, from this skill's folder, reports every pattern-matchable tell with its line number and its fix, plus the word count. Its output *is* the word-list half of this skill, which is why that half is not in this file. So do not open `references/tells.md` when the checker ran: you would be pulling the whole list back into context for the rest of the task to read what the output already told you. Open it only if the checker failed to run, or if a specific hit needs the reasoning behind its one-line fix.
4. **Fix the strict hits, judge the candidates.** Strict means near-zero false positives. Candidate means usually a tell, sometimes the project's real vocabulary, where swapping it makes the prose worse. That call is yours, not the script's.
5. **Work the [judgment tells](#judgment-tells).** No regex finds them, so this is where a rushed pass loses the most.
6. **Add voice**, if the register allows it. See [Voice](#voice).
7. **Self-audit, then re-run the checker.** Ask "what still reads as AI-generated?" and fix what the answer names. The re-run prints the new word count, which you compare against the original *minus your markers*, since a marked draft can be longer than what ships. Discounting those, an unslopped draft is nearly always shorter; if it still grew, you added explanation where you should have cut decoration.
8. **Report what you did.** See [Report](#report).

## Register

Four registers, because the voice advice below is actively wrong for two of them.

| Register | Covers | Voice |
|---|---|---|
| Reference | README, API and config docs, changelog, spec | None. The reader wants the fact and wants to leave. |
| Argument | Blog post, essay, design doc, ADR, PR description, release notes, commit body | Expected. Say what you think and why. |
| Conversation | Chat reply, email, review comment, issue comment | Expected, plus contractions and short sentences. |
| Instruction | A skill, `AGENTS.md`, `CLAUDE.md`, a reference an agent reads | None. Imperative throughout. See [Not this skill](#not-this-skill). |

A commit body is Argument, not Reference: it argues that this was the right fix, which is an opinion the reader needs. A changelog entry only reports what moved. Two conventions ride along with it. Write what the change *does* in the imperative (`Move the retry policy`, never `I moved the retry policy`), because the diff already narrates your actions and the body is for the intent behind them; first person belongs to the reasoning, not the inventory. And keep inference markers out of the message: a commit body ships the moment you save it, so anything you want the author to read first goes in the report instead.

Every tell applies to all four. Voice applies to Argument and Conversation only. "I find this approach elegant" in an API reference is its own kind of slop, the writer performing personality where the reader wanted a signature.

When the register is ambiguous, ask. If you cannot ask, pick Reference: over-neutral prose is a smaller failure than a config doc with a personality.

## Leave alone

Editing prose is safe. Editing claims is not, and the way this skill fails is a confident rewrite that reads better and says something the author never meant.

- **Code and code-shaped text.** Fenced blocks, inline spans, identifiers, config keys, flags, paths, log output, error strings. The checker masks all of it before matching; do the same by eye.
- **Someone else's words.** A quotation, a citation, a transcript, a changelog reporting what upstream said. Slop inside a quotation is evidence and smoothing it destroys the evidence, so this outranks every tell: leave the dashes, the `delve`, all of it. The quote *marks* are your document's typography rather than the source's, so straightening those is fine. Nothing between them changes.
- **Terms of art.** The abstract-noun tells are tells only where a plainer concrete word exists. Where a project genuinely calls the thing a harness, a surface, or a primitive, that *is* the concrete word, and renaming it costs the reader the ability to search for it. The checker flags all three as candidates so the decision reaches you.
- **A written style guide.** If the project has one, it outranks this list. Read it first.

### When a claim is missing its evidence

Never change what a sentence claims in service of how it reads. When a claim has no source, or names a result without the mechanism behind it, you have three moves:

- **Leave it and flag it**, when the author probably has the source and you do not.
- **Cut it and say so in your report**, when the claim is not about this document's subject anyway.
- **Write the missing explanation and mark it as yours in place**, so the author confirms or deletes it: `<!-- inferred: a fixed delay makes every caller retry at the same instant. Confirm or cut. -->`, or whatever marker the format allows. Keep it to one line. A marker that runs to a paragraph makes the document longer than the draft you were sent, which is the opposite of the job, and the reader stops seeing them as annotations.

Reach for the third most often. A page that explains the mechanism beats a page that flags its absence, and the marker means nothing lands under the author's name that they did not say. The form of the marker does not matter; its visibility does.

Budget one or two markers per document, for the claims that carry the piece. Past that they stop being annotations and become a second document arguing with the first, and the author reads neither. If a draft seems to want five, it is not missing sentences, it is missing a conversation: say so in the report and leave the rest alone.

Forbidden is the quiet version, where a claim changes, an explanation appears, and nobody is told.

## Judgment tells

Ranked by yield. No regex finds these, which is why they are here rather than in `references/tells.md` with the ones that reach you through the checker's output.

### 1. Nothing said

The strongest tell, and ungreppable. Ask what a sentence tells the reader to do or know. If you cannot restate it as an instruction, a fact, or a number, cut it.

"the database stays close at hand", "types that follow your schema" name a feeling. The fixes name a mechanism: "`.toSQL()` returns the exact string sent to the database", "a column rename fails the build".

Then the brutal version: if the sentence could appear unchanged in another project's docs, it says nothing about this one.

### 2. Conclusions nobody earned

- **The superficial -ing phrase** bolted to a sentence's end: `ensuring scalability`, `reflecting a broader shift`, `fostering collaboration`. Each attaches a conclusion to a fact without earning it. Delete it, or replace it with the actual reason.
- **Name-dropping.** Listing outlets or companies without saying what any of them said. Pick one and quote it.
- **The formulaic challenge.** `Despite challenges, the project continues to thrive.` Say what the challenge was and what happened.

### 3. Emphasis with nothing behind it

- **The rule of three.** Use the number you actually have, even when it is two.
- **False ranges.** "from X to Y" where X and Y sit on no shared scale. List the things.

### 4. Missing actors, and shapes that read as generated

- **Passive with a missing actor.** Catch `is/are/was/were` plus a past participle and name who acts: `queries are validated` becomes `the compiler validates queries`. Passive is fine when the actor is unknown or does not matter, and inventing one to satisfy this rule breaks [Leave alone](#leave-alone).
- **Dense sentences.** If the reader backtracks to parse it, split it or drop a clause.
- **Uniform rhythm.** Every sentence the same length reads mechanical. Varying it is mostly a byproduct of splitting the dense ones.
- **Mechanically parallel bullets.** Every item the same shape and length is a list generated rather than written. Let items differ, or make it prose.

### 5. Close to the checker, past what it can settle

- **Synonym cycling.** `protagonist`, `main character`, `central figure`, `hero`, one paragraph. Pick one and repeat it; repetition is clearer than variety here.
- **Colons as mid-sentence connectors.** Fine before a list or example. "If you're coming from traditional automation: instead of registering handlers, you describe conditions" gains nothing from the colon. Rewrite so the point stands without the comparison.
- **Sycophancy.** `Great question`, `You're absolutely right`. Answer instead.

## Voice

Argument and Conversation only. This half is where a de-slopped draft stops reading like a form.

- **Have an opinion.** React to the facts instead of weighing pros and cons equally. A reader can tell when the writer has no stake.
- **Acknowledge the complication.** "Impressive, and slightly unsettling" beats "impressive". Anything with only good sides was not examined.
- **Use "I" where it fits.** First person is how you take responsibility for a claim.
- **Be concretely specific.** Not "this is concerning" but "there is something unsettling about agents churning away at 3am".
- **Let some mess in.** A sentence that starts somewhere unexpected, a paragraph that runs short. Perfect symmetry is the machine's signature.

Voice is not volume. None of this licenses exclamation marks, jokes nobody asked for, or an opinion about something you did not examine.

## Report

Three lists. The last two are the ones that matter.

- **Fixed.** The tells you cut, a line each.
- **Left alone deliberately.** What you held back under [Leave alone](#leave-alone), with the reason. This is how the author catches you being wrong about their vocabulary.
- **Flagged, not changed.** Claims with no source, results with no mechanism, and every inference you marked in place. This is where you used judgment on their behalf, so it is where you are most likely to have been wrong.

Give the before and after word count, measured on the document without your markers, since a marked draft can be longer than what ships. `references/tells.md` ends with a worked example of all three lists, there for a human reading this skill rather than for you to open mid-task.

## No draft yet

This is an edit pass and it needs prose to edit. Starting from nothing, three parts still apply: set the register before you write, keep [Leave alone](#leave-alone) in view so you do not draft a claim you cannot source, and aim at [Nothing said](#1-nothing-said) rather than fixing it afterwards. For getting from raw notes to a draft, that is `writing-fragments` and `writing-shape`.

## Not this skill

For a document an *agent* reads, call the Skill tool with "writing-for-agents" first.

The two are layered, not alternatives, and they split by what they own. `writing-for-agents` owns the structure: what goes in the file, what a pointer earns, whether the instructions hold across runs. This skill owns the prose inside that structure, which is the Instruction register above. Structure first, because it decides what text exists, then this on what survives.
