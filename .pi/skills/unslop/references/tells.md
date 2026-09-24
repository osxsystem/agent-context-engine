# Tells a pattern can match

This is the half of the list `scripts/check-tells.py` finds for you. Read it when the checker cannot run, or when you want the reasoning behind a hit rather than the one-line fix its output prints.

Each heading names the checker's own pattern names, so a hit maps straight back here. **Strict** means near-zero false positives, so fix every one. **Candidate** means usually a tell and sometimes the project's real vocabulary, which is a call only you can make. Before swapping any word, read the Leave alone section of [SKILL.md](../SKILL.md): in the right project several of these are the correct term.

The tells no regex can find, led by the biggest one, are in `SKILL.md` itself.

- [Puffery and promotion](#puffery-and-promotion)
- [Unsourced authority](#unsourced-authority)
- [Manufactured emphasis](#manufactured-emphasis)
- [Formula and filler](#formula-and-filler)
- [Weak verbs](#weak-verbs)
- [Word choice](#word-choice)
- [Punctuation and layout](#punctuation-and-layout)
- [Chat artifacts](#chat-artifacts)
- [Worked example](#worked-example)

## Puffery and promotion

`puffery` (strict), `promo-adjective` (candidate)

`pivotal moment`, `testament to`, `evolving landscape`, `setting the stage for`, `indelible mark`, `deeply rooted`.

Travel-brochure dialect: `nestled`, `vibrant`, `breathtaking`, `renowned`, `stunning`, `must-visit`.

Software dialect: `seamless`, `robust`, `powerful`, `battle-tested`, `production-ready`, `groundbreaking`, `best-in-class`, `cutting-edge`, `enterprise-grade`, `turnkey`, `effortless`, `out of the box`.

Say what it does, or how fast, or what broke before it existed.

## Unsourced authority

`vague-attribution` (strict)

`Experts believe`, `Industry reports suggest`, `Studies show`, `Some critics argue`. Name the source or delete the claim. Do not paraphrase it vaguer, which is the same claim with the evidence filed off.

Deleting is only safe when you say you deleted it. See "When a claim is missing its evidence" in [SKILL.md](../SKILL.md) for the three moves and when each one is right.

## Manufactured emphasis

`not-just-x-but-y` (strict), `fragment-triad` (candidate)

- **The negated pivot.** `not just X, but Y`, `it isn't only X, it's Y`. State the point directly.
- **The fragment triad.** `Not a library. A philosophy. A way of life.` Write one real sentence.

Its two ungreppable cousins, the rule of three and the false range, are in `SKILL.md`.

## Formula and filler

`filler-phrase` (strict), `stacked-hedge` (strict), `generic-conclusion` (strict), `stock-opener` (strict), `section-restatement` (candidate)

- **Filler phrases.** `In order to` is `to`. `Due to the fact that` is `because`. `It is important to note that` is nothing. `When it comes to` is nothing.
- **Stacked hedging.** `could potentially possibly be argued that it might` is `may`. One hedge, or none.
- **Generic conclusion.** `The future looks bright`, `only time will tell`, `the possibilities are endless`. State a specific plan or fact, or end one sentence earlier.
- **Stock opener.** `In today's fast-paced landscape`, `Let's dive in`, `Here's the thing`, `Think of it as`. Start at the first real sentence.
- **Section restatement.** `In short`, `The takeaway`, `At its core`, `Key takeaways`. A section needing a summary is a section that is too long.

## Weak verbs

`fancy-is` (candidate), `weak-adverb` (candidate)

- **Fancy ways to say "is".** `serves as`, `stands as`, `boasts`, `features`. Say `is` or `has`.
- **Adverbs propping up weak verbs.** `runs quickly` is `is fast`, or the number. `significantly improves` is the measured delta. An adverb holding up a verb means the verb is wrong. Where the number is the fix and you do not have the number, that is a flag rather than a deletion.

The third member of this group, passive voice with the actor missing, needs judgment about who actually acts, so it is in `SKILL.md`.

## Word choice

`ai-vocabulary` (candidate), `fancier-synonym` (candidate), `abstract-metaphor-noun` (candidate)

- **AI vocabulary.** `additionally`, `crucial`, `delve`, `enduring`, `enhance`, `foster`, `garner`, `interplay`, `intricate`, `landscape` (abstract), `moreover`, `furthermore`, `myriad`, `pivotal`, `realm`, `showcase`, `showcasing`, `tapestry` (abstract), `testament`, `underscore`, `unlock`, `elevate`, `empower`, `navigate` (abstract). Plain words instead.
- **The fancier synonym.** `utilize` is `use`. `leverage` is `use`. `facilitate` is `help`. `numerous` is `many`. `in the event that` is `if`. `prior to` is `before`. The longer word is rarely clearer.
- **Abstract metaphor nouns.** `substrate`, `wedge`, `vector`, `locus`, `vantage`, `nexus`, `primitive`, `harness`, `surface`, `bedrock`, `scaffolding`, `modality`, `paradigm`, `gold-plating`, `ratchet`, `flywheel`, `north star`, `endgame`, `evacuate` (for moving code). `Substrate` is `base`. `Wedge in` is `add`. `Vector` is `way` or `method`. `Gold-plating` is `more than the job needs`. `Ratchet` is the mechanism's real name, or `a limit that only tightens`. `Evacuate` is `move out`.

`harness`, `surface` and `primitive` are the three most likely to be a project's real vocabulary. The checker flags them as candidates so the decision reaches you.

## Punctuation and layout

`em-dash`, `en-dash`, `hyphen-as-dash`, `curly-quote`, `decorative-symbol` (all strict); `spaced-hyphen`, `callout-spam`, `title-case-heading`, `restating-bold-label`, `question-heading` (candidate)

- **Em dashes.** Avoid them, and reach for what the sentence actually wants: a period where the clause is really a second sentence, a semicolon between two independent clauses, a comma around an appositive, a colon before a list or example, or the conjunction the dash was hiding (`because`, `so`, `since`, `and`). What fails is swapping one separator for another wherever the dash sat. An en dash, a hyphen, a double hyphen, or a parenthesis dropped in mechanically is the same tell in a different costume, and the giveaway is that the sentence still has the dash's shape. Parentheses earn their place around a genuine aside, one the sentence could lose without missing it; they are not a dash in disguise.
- **Boldface.** Not on every proper noun and acronym. Bold is for the one word a skimming reader must not miss.
- **Inline-header lists.** The tell is a bold label plus colon that restates the line: "**Performance:** Performance improved". Convert to prose. A bold lead-in ending in a period, naming the item, followed by new detail ("**Schema in TypeScript.** Tables live in one file.") is a legitimate shape, and so is a glossary defining a named field.
- **Title case headings.** Sentence case, unless every capital is a proper noun.
- **Question headings.** Fine in a real FAQ, a tell when they are rhetorical.
- **Decorative emoji and check marks.** Remove from headings, bullets and tables.
- **Curly quotes.** Straight quotes, except inside a quotation, where the source's words are untouchable and only the marks around them are yours.
- **Callout spam.** "Note:", "Important:", "Tip:" stacked down a page. If it matters, put it in the sentence that needs it.

## Chat artifacts

`chatbot-phrase` (strict), `cutoff-disclaimer` (strict)

- **Chatbot phrases.** `I hope this helps`, `Let me know if`, `Of course!`, `Certainly!`, `Found the smoking gun!`, `Ready to get started?`. Delete.
- **Cutoff disclaimers.** `While specific details are limited`, `based on publicly available information`. Find the source or cut the sentence.

Sycophancy belongs to this family and is in `SKILL.md`, because the phrasings are unbounded.

## Worked example

The point is the judgment, not the word swaps. Notice that the unsourced claim is flagged rather than fixed, that the missing mechanism is written but marked, and that the result is 40% shorter.

Before:

```text
## Unlocking Seamless Data Sync

In today's fast-paced development landscape, keeping state consistent across
devices is crucial. Our new sync engine serves as a testament to what's
possible — it's not just a cache, but a robust, battle-tested foundation for
the future. Industry reports suggest that offline-first apps see significantly
better retention. The future looks bright.
```

After:

```text
## Offline sync

Sync writes to a local SQLite file first, then pushes to the server when the
connection returns. Conflicts resolve last-write-wins per row, keyed on
`updated_at`. Writes made offline survive an app kill.

<!-- inferred: last-write-wins is why a row edited on two devices keeps the
later `updated_at` and silently drops the earlier edit. Confirm or cut. -->
```

Reported back to the author:

```text
Fixed: title-case heading, stock opener, "crucial", "serves as a testament",
em dash, not-just-X-but-Y, "robust"/"battle-tested", "significantly",
generic conclusion. Replaced the feeling-words with the mechanism and the
guarantee. 61 words down to 38.

Left alone deliberately: nothing here.

Flagged, not changed:
- "Industry reports suggest offline-first apps see better retention." No
  source, and I cannot verify it. Name the report or cut the sentence.
- I wrote the consequence of last-write-wins and marked it inline, because
  the page states the rule without saying what it costs. Confirm it or
  delete the comment.
```
