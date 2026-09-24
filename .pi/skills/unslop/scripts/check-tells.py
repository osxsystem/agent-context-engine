#!/usr/bin/env python3
"""Find the mechanical AI tells in prose, so judgment can be spent elsewhere.

About a third of the tells in `unslop`'s SKILL.md are pure pattern matching:
em dashes, curly quotes, a fixed vocabulary, a handful of fixed constructions.
An agent hunting those by eye is slow and unreliable, and every miss looks like
the skill did nothing. A script never misses them, so the agent's attention goes
to register, specificity, and voice, which no regex can judge.

Two tiers, because a linter that cries wolf gets ignored:

  strict     near-zero false positives. Fix every hit.
  candidate  a word or shape that is usually a tell. The agent decides each
             one: some are the domain's real vocabulary, and replacing those
             makes the prose worse.

Code is never prose. Fenced blocks, inline spans, link targets, and HTML
comments are masked with spaces before matching, which keeps line and column
numbers true while making the content invisible. That is also why a worked
before/after example inside a fenced block can carry the tells it teaches.

No third-party dependencies: the checker has to run wherever python3 does,
including inside CI and inside an agent's sandbox.

Usage:
    check-tells.py FILE_OR_DIR...        human-readable report
    check-tells.py --json FILE...        machine-readable, for scripted checks
    check-tells.py --summary DIR         counts per file, no individual hits
    check-tells.py --fail-on candidate   exit 1 on candidates too (default: strict)
"""
import argparse
import json
import os
import re
import sys

STRICT = "strict"
CANDIDATE = "candidate"

# --- vocabulary -------------------------------------------------------------

AI_VOCABULARY = [
    "additionally", "crucial", "delve", "enduring", "enhance", "fostering",
    "garner", "interplay", "intricate", "landscape", "pivotal", "showcase",
    "showcasing", "tapestry", "testament", "underscore", "underscores",
    "moreover", "furthermore", "myriad", "realm", "unlock",
    "unlocking", "elevate", "empower", "navigate", "navigating",
]

PROMO_ADJECTIVES = [
    "seamless", "seamlessly", "robust", "powerful", "blazing-fast",
    "battle-tested", "production-ready", "best-in-class", "cutting-edge",
    "state-of-the-art", "game-changing", "groundbreaking", "revolutionary",
    "effortless", "effortlessly", "turnkey", "enterprise-grade", "world-class",
    "nestled", "vibrant", "breathtaking", "renowned", "stunning", "must-visit",
]

PLAIN_WORD_SWAPS = [
    "utilize", "utilizes", "utilizing", "leverage", "leverages", "leveraging",
    "facilitate", "facilitates", "numerous", "commence", "endeavor",
    "prior to", "subsequent to", "in the event that", "a plethora of",
]

# The three most likely to be a project's real vocabulary (harness, surface,
# primitive) sit here deliberately: candidate tier puts the decision in front
# of the agent instead of letting a regex rename something searchable.
ABSTRACT_METAPHOR_NOUNS = [
    "substrate", "wedge", "vector", "locus", "vantage", "nexus", "primitive",
    "primitives", "harness", "surface", "surfaces", "bedrock", "scaffolding",
    "modality", "paradigm", "gold-plating", "ratchet", "north star",
    "flywheel", "endgame", "evacuate", "evacuating",
]

WEAK_ADVERBS = [
    "significantly", "substantially", "dramatically", "incredibly",
    "remarkably", "truly", "simply", "essentially", "fundamentally",
    "arguably", "notably", "seamlessly", "effortlessly",
]

# --- patterns ---------------------------------------------------------------
# (name, tier, compiled regex, one-line explanation of the fix)

PATTERNS = [
    ("em-dash", STRICT, re.compile(r"—"),
     "pick what the sentence wants: period, semicolon, comma, colon, or the "
     "conjunction the dash hid. Swapping in another separator keeps the tell"),
    ("en-dash", STRICT, re.compile(r"–"),
     "an en dash as a connector is the same tell wearing a smaller hat"),
    ("hyphen-as-dash", STRICT, re.compile(r"(?<=\w)\s--\s(?=\w)"),
     "a double hyphen standing in for a dash is still a dash"),
    ("spaced-hyphen", CANDIDATE, re.compile(r"(?<=\w) - (?=\w)"),
     "reads as a dash substitute unless it is a range or a literal"),
    ("curly-quote", STRICT, re.compile(r"[‘’“”]"),
     "use straight quotes; inside a quotation only the marks are yours to change"),
    ("decorative-symbol", STRICT, re.compile(
        r"[☀-➿⬀-⯿\U0001f000-\U0001faff‼⁉]️?"),
     "drop it; arrows used as real notation are not matched"),
    ("not-just-x-but-y", STRICT, re.compile(
        r"\b(?:not|isn't|isn`t|is not|aren't|wasn't)\s+(?:just|only|merely)\b[^.!?\n]{1,70}?\bbut\b",
        re.I),
     "state the point directly"),
    ("chatbot-phrase", STRICT, re.compile(
        r"\b(?:i hope this helps|let me know if|feel free to reach out|happy to help"
        r"|of course!|certainly!|absolutely!|great question|you'?re absolutely right"
        r"|excellent point|you'?re spot on|smoking gun)\b", re.I),
     "delete; this is conversation residue, not content"),
    ("cutoff-disclaimer", STRICT, re.compile(
        r"(?:while specific details (?:are|remain) limited|as of my (?:last|knowledge)"
        r"|based on (?:publicly )?available information|i don'?t have access to real-?time)",
        re.I),
     "find the source or cut the sentence"),
    ("generic-conclusion", STRICT, re.compile(
        r"(?:the future looks bright|only time will tell|the possibilities are endless"
        r"|the sky'?s the limit|one thing is clear|it remains to be seen"
        r"|ready to get started\?)", re.I),
     "state a specific plan, fact, or number, or end the piece one sentence earlier"),
    ("stock-opener", STRICT, re.compile(
        r"(?:in today'?s (?:[a-z-]+\s+){1,3}(?:world|landscape|environment|era|climate)"
        r"|in an era of|in the world of|let'?s dive in|let'?s take a look"
        r"|here'?s the thing|think of it as|imagine (?:a|an|for a moment))", re.I),
     "start at the first real sentence instead"),
    ("section-restatement", CANDIDATE, re.compile(
        r"^\s*(?:\*\*)?(?:in short|in summary|to sum up|all in all|at its core"
        r"|the takeaway|bottom line|key takeaways?)\b", re.I | re.M),
     "if the section needs a summary the section is too long"),
    ("vague-attribution", STRICT, re.compile(
        r"(?:experts (?:believe|say|agree|suggest)|studies show|research shows"
        r"|industry reports? (?:suggest|show|indicate)|some (?:critics|observers|argue)"
        r"|many believe|it is widely (?:believed|regarded|considered))", re.I),
     "name the source or delete the claim; do not paraphrase it into vagueness"),
    ("puffery", STRICT, re.compile(
        r"(?:pivotal moment|testament to|evolving landscape|setting the stage for"
        r"|indelible mark|deeply rooted|rich (?:history|tapestry))", re.I),
     "state what happened"),
    ("fancy-is", CANDIDATE, re.compile(r"\b(?:serves as|stands as|boasts)\b", re.I),
     "say \"is\" or \"has\""),
    ("filler-phrase", STRICT, re.compile(
        r"(?:in order to|due to the fact that|it is important to note that"
        r"|it'?s worth noting that|needless to say|at the end of the day"
        r"|when it comes to|the fact of the matter is)", re.I),
     "\"to\", \"because\", or nothing at all"),
    ("stacked-hedge", STRICT, re.compile(
        r"\b(?:could potentially|may possibly|might arguably|it could be argued"
        r"|there is a possibility that|somewhat of a)\b", re.I),
     "one hedge or none"),
    ("callout-spam", CANDIDATE, re.compile(
        r"^\s*(?:>\s*)?(?:\*\*)?(?:Note|Important|Tip|Warning|Caution|Remember)(?:\*\*)?:",
        re.M),
     "if it matters, put it in the sentence that needs it"),
    ("ai-vocabulary", CANDIDATE,
     re.compile(r"\b(?:%s)\b" % "|".join(AI_VOCABULARY), re.I),
     "replace with a plain word, unless it is this project's actual term"),
    ("promo-adjective", CANDIDATE,
     re.compile(r"\b(?:%s)\b" % "|".join(PROMO_ADJECTIVES), re.I),
     "describe the mechanism or the number instead of praising it"),
    ("fancier-synonym", CANDIDATE,
     re.compile(r"\b(?:%s)\b" % "|".join(PLAIN_WORD_SWAPS), re.I),
     "use, help, many, if"),
    ("abstract-metaphor-noun", CANDIDATE,
     re.compile(r"\b(?:%s)\b" % "|".join(ABSTRACT_METAPHOR_NOUNS), re.I),
     "pick the concrete word, unless the project really calls it this"),
    ("weak-adverb", CANDIDATE,
     re.compile(r"\b(?:%s)\b" % "|".join(WEAK_ADVERBS), re.I),
     "a stronger verb or the measured number"),
    ("question-heading", CANDIDATE, re.compile(r"^#{1,6} .*\?\s*$", re.M),
     "fine in a real FAQ, a tell when it is rhetorical"),
]

HEADING_RE = re.compile(r"^(#{1,6})\s+(.*?)\s*$")
# Both spellings of the inline header: `**Label:**` and `**Label**:`.
BOLD_LABEL_RE = re.compile(r"^\s*(?:[-*+]\s+)?\*\*([^*:]{1,60})(?::\*\*|\*\*:)\s*(.+)$")
WORD_RE = re.compile(r"[A-Za-z][A-Za-z'’-]*")
TITLE_WORD_RE = re.compile(r"^[A-Z][a-z]{2,}$")
# Words a title-case heading may capitalise without being title case.
SENTENCE_CASE_EXEMPT = frozenset({
    "i", "a", "an", "the", "and", "or", "but", "for", "to", "of", "in", "on",
    "at", "by", "with", "from", "as", "is", "it",
})


def mask_non_prose(text):
    """Blank out code, links, and comments, preserving every offset.

    Replacing with spaces rather than deleting is what keeps reported line and
    column numbers pointing at the real character in the file.
    """
    chars = list(text)

    def blank(start, end):
        for i in range(start, min(end, len(chars))):
            if chars[i] != "\n":
                chars[i] = " "

    # Fenced blocks: ``` or ~~~ at the start of a line, closed by the same marker.
    fence = None
    pos = 0
    for line in text.splitlines(keepends=True):
        stripped = line.lstrip()
        marker = None
        if stripped.startswith("```"):
            marker = "```"
        elif stripped.startswith("~~~"):
            marker = "~~~"
        if fence is None and marker:
            fence = marker
            blank(pos, pos + len(line))
        elif fence is not None:
            blank(pos, pos + len(line))
            if marker == fence:
                fence = None
        pos += len(line)

    masked = "".join(chars)
    for pattern in (
        re.compile(r"`[^`\n]+`"),            # inline code
        re.compile(r"\]\([^)\n]*\)"),        # link and image targets
        re.compile(r"<https?://[^>\s]+>"),   # autolinks
        re.compile(r"(?<![\w(])https?://\S+"),  # bare URLs
        re.compile(r"<!--.*?-->", re.S),     # HTML comments
        re.compile(r"^\s{4,}\S.*$", re.M),   # indented code blocks
    ):
        for m in pattern.finditer(masked):
            blank(m.start(), m.end())
        masked = "".join(chars)
    return masked


def line_col(text, index):
    line = text.count("\n", 0, index) + 1
    col = index - (text.rfind("\n", 0, index) + 1) + 1
    return line, col


def find_title_case_headings(masked):
    """A heading is title case when most of its non-leading words are capitalised.

    Proper nouns break any purely mechanical version of this check, which is why
    it reports as a candidate: `Kotlin Multiplatform Setup` is title case,
    `Kotlin Multiplatform` is a product name.
    """
    hits = []
    for m in re.finditer(r"^#{1,6}\s+(.*?)\s*$", masked, re.M):
        words = WORD_RE.findall(m.group(1))
        if len(words) < 3:
            continue
        rest = [w for w in words[1:] if w.lower() not in SENTENCE_CASE_EXEMPT]
        if not rest:
            continue
        capped = [w for w in rest if TITLE_WORD_RE.match(w)]
        if len(capped) >= 2 and len(capped) / len(rest) >= 0.6:
            hits.append((m.start(1), m.group(1)))
    return hits


def find_restating_labels(masked):
    """The inline-header tell: a bold label, a colon, then the label again.

    A bold lead-in that ends in a period and is followed by genuinely new detail
    is a legitimate shape, so only the colon form is examined, and only when a
    substantial word from the label reappears immediately after it.

    Two shapes share that surface and are not the tell, so they are skipped:
    a glossary of field names (`**retry_count**: number of retries`), where
    repeating the name is the definition, and a catalog entry whose label is a
    link. What survives still needs a human read, which is why this reports as
    a candidate rather than strict.
    """
    hits = []
    for m in re.finditer(r"^.*$", masked, re.M):
        line = m.group(0)
        parsed = BOLD_LABEL_RE.match(line)
        if not parsed:
            continue
        label = parsed.group(1)
        if re.search(r"[\[\]_(]", label) or re.match(r"^[a-z]+[A-Z]", label):
            continue
        label_words = {w.lower() for w in WORD_RE.findall(label) if len(w) > 3}
        opening = [w.lower() for w in WORD_RE.findall(parsed.group(2))[:4]]
        if label_words & set(opening):
            hits.append((m.start(), line.strip()[:80]))
    return hits


def find_fragment_triads(masked):
    """Three very short sentences in a row: "Not a tool. A partner. A teammate."

    Staccato emphasis is a current tell, and three consecutive sub-five-word
    sentences is the shape it almost always takes.
    """
    hits = []
    for m in re.finditer(
        r"(?<![.\w])((?:[A-Z][^.!?\n]{2,34}[.!?]\s+){2}[A-Z][^.!?\n]{2,34}[.!?])",
        masked,
    ):
        parts = re.split(r"(?<=[.!?])\s+", m.group(1).strip())
        if len(parts) == 3 and all(len(WORD_RE.findall(p)) <= 5 for p in parts):
            hits.append((m.start(1), m.group(1)[:80]))
    return hits


CUSTOM_CHECKS = [
    ("title-case-heading", CANDIDATE, find_title_case_headings,
     "use sentence case, unless every capital is a proper noun"),
    ("restating-bold-label", CANDIDATE, find_restating_labels,
     "convert to prose, unless the label names a field and the line defines it"),
    ("fragment-triad", CANDIDATE, find_fragment_triads,
     "one real sentence beats three fragments performing emphasis"),
]


def check_text(text):
    masked = mask_non_prose(text)
    hits = []
    for name, tier, pattern, fix in PATTERNS:
        for m in pattern.finditer(masked):
            line, col = line_col(masked, m.start())
            hits.append({
                "tell": name, "tier": tier, "line": line, "column": col,
                "match": m.group(0).strip()[:80], "fix": fix,
            })
    for name, tier, finder, fix in CUSTOM_CHECKS:
        for index, snippet in finder(masked):
            line, col = line_col(masked, index)
            hits.append({
                "tell": name, "tier": tier, "line": line, "column": col,
                "match": snippet, "fix": fix,
            })
    hits.sort(key=lambda h: (h["line"], h["column"], h["tell"]))
    return hits, len(masked.split())


def collect_paths(paths):
    out = []
    skip = {".git", "node_modules", ".venv", "__pycache__", ".beads"}
    for path in paths:
        if os.path.isfile(path):
            out.append(path)
            continue
        if not os.path.isdir(path):
            print(f"check-tells: no such file or directory: {path}", file=sys.stderr)
            continue
        for root, dirs, files in os.walk(path):
            dirs[:] = sorted(d for d in dirs if d not in skip)
            out.extend(os.path.join(root, f) for f in sorted(files)
                       if f.endswith((".md", ".markdown", ".txt")))
    return out


def main():
    parser = argparse.ArgumentParser(
        description="Report the mechanically detectable AI tells in prose.")
    parser.add_argument("paths", nargs="+", help="files, or directories to walk for markdown")
    parser.add_argument("--json", action="store_true", help="emit JSON instead of a report")
    parser.add_argument("--summary", action="store_true", help="per-file counts only")
    parser.add_argument("--fail-on", choices=["strict", "candidate", "never"],
                        default="strict", help="which tier sets exit code 1 (default: strict)")
    args = parser.parse_args()

    results = []
    for path in collect_paths(args.paths):
        try:
            with open(path, encoding="utf-8") as fh:
                text = fh.read()
        except (OSError, UnicodeDecodeError) as exc:
            print(f"check-tells: cannot read {path}: {exc}", file=sys.stderr)
            continue
        hits, words = check_text(text)
        results.append({"path": path, "words": words, "hits": hits})

    totals = {STRICT: 0, CANDIDATE: 0}
    for result in results:
        for hit in result["hits"]:
            totals[hit["tier"]] += 1

    if args.json:
        print(json.dumps({"files": results, "totals": totals}, indent=2))
    else:
        for result in results:
            counts = {STRICT: 0, CANDIDATE: 0}
            for hit in result["hits"]:
                counts[hit["tier"]] += 1
            if not result["hits"]:
                if not args.summary:
                    print(f"{result['path']}: clean ({result['words']} words)")
                continue
            print(f"\n{result['path']}  "
                  f"{counts[STRICT]} strict, {counts[CANDIDATE]} candidate, "
                  f"{result['words']} words")
            if args.summary:
                continue
            for tier in (STRICT, CANDIDATE):
                tier_hits = [h for h in result["hits"] if h["tier"] == tier]
                if not tier_hits:
                    continue
                print(f"  {tier}")
                for hit in tier_hits:
                    where = f"{hit['line']}:{hit['column']}"
                    print(f"    {where:>9}  {hit['tell']:<22} {hit['match']!r}")
                    print(f"    {'':>9}  {'':<22} -> {hit['fix']}")
        print(f"\ntotal: {totals[STRICT]} strict, {totals[CANDIDATE]} candidate "
              f"across {len(results)} file(s)")

    if args.fail_on == "never":
        return 0
    if totals[STRICT]:
        return 1
    if args.fail_on == "candidate" and totals[CANDIDATE]:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
