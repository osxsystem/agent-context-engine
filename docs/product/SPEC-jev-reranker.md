# SPEC: Adopt Jev as the reranker

**Derived from:** `docs/product/PRD-jev-reranker.md`
**Repository:** `osxsystem/agent-context-engine`
**Author:** Do Viet Hung

---

## Problem Statement

When the owner asks the Context Engine for code, the results are ordered by a
general-purpose chat model that was asked, in prose, to reply with a JSON
object. Two things go wrong with that arrangement.

First, the ordering is a side effect of text generation. The model is not
scoring relevance; it is writing a document that happens to imply an order. The
JSON contract is enforced by wording rather than by a schema, so the code
carries a repair loop for malformed replies and a retry budget for when the
repair fails. There is no score to inspect, no threshold to tune, and no way to
tell a confident ranking from a coin flip.

Second, the same reply is asked to narrow each chunk to the interesting lines.
The model invents line numbers. Those numbers become the `path#Lstart-end`
headers the owner reads, and nothing checks them against the structure of the
file — the only safeguards are a two-line pad, a clamp to the chunk's own
bounds, and a floor that skips narrowing on short chunks. A returned range can
begin in the middle of a function and end before it closes.

Underneath both problems sits a third: nobody knows whether the reranker helps.
The repo's retrieval benchmark deliberately runs with reranking switched off,
so the stage has never been measured here.

## Solution

Rerank with a model built for the job. TypeSafe's Jev answers typed questions
with calibrated probabilities instead of generating text, so ranking becomes a
number the code can sort on, store, and threshold — with no parsing step to
fail.

For each candidate chunk the engine asks one question that decides its rank
(*does this code implement or define what the query asks for?*) and, in the same
request, one question per symbol that overlaps the chunk (*does this particular
symbol contain it?*). Narrowing stops being generation and becomes selection:
the returned range is a real symbol span the engine already knows about, not a
guess. Chunks with no clear span come back whole.

The owner picks this from the same Settings panel they use today. The Provider
dropdown is relabelled **Rerank provider** — an honest name, since every control
beside it already governs reranking — and gains a **TypeSafe (Jev)** option with
its own API key list. Existing installs are untouched; the migration carries the
current provider forward.

It ships switched off. Before it becomes the default, a new benchmark runs both
rerankers over the owner's corpus and reports recall side by side. If recall@1
regresses, or neither recall@5 nor recall@10 improves, or the stage gets slower,
the default does not move.

Repo chat and agentic RAG keep using the existing chat model. Jev cannot
generate text or call tools, so this is a boundary, not a gap.

## User Stories

**Configuring**

1. As the owner, I want a **TypeSafe (Jev)** option in the rerank provider
   dropdown, so that I can enable it without hand-editing a settings file.
2. As the owner, I want the provider dropdown labelled **Rerank provider**, so
   that I stop wondering whether it also governs chat.
3. As the owner, I want a TypeSafe API key list that behaves like the existing
   one — add, show, remove — so that key management works the way I already
   expect.
4. As the owner, I want my TypeSafe keys stored separately from my existing
   provider keys, so that a Google key is never sent to TypeSafe or the reverse.
5. As the owner, I want the model field to default to the vendor's stable alias
   when I select Jev, so that I don't have to look up a version string.
6. As the owner, I want the speed hint beside the model field to stop
   recommending a Google model when Jev is selected, so that the guidance
   matches the provider.
7. As the owner, I want every new label and hint translated into all three
   shipped languages, so that the settings page is not half-English.
8. As the owner, I want to point the Jev client at a different base URL, so that
   I can route it through a local proxy the way I already do for my chat model.

**Upgrading**

9. As the owner, I want my existing configuration to keep working untouched
   after upgrading, so that an upgrade is never a re-configuration.
10. As the owner running a custom OpenAI-compatible endpoint, I want the
    migration to carry *that* provider forward — not the packaged default — so
    that my working setup is not replaced by one I never chose.
11. As the owner, I want the settings file to migrate forward automatically on
    first boot, so that I never edit JSON by hand.

**Retrieving**

12. As a developer using an MCP client, I want the most relevant chunk ranked
    first, so that the agent spends its first turn on the right code.
13. As a developer, I want returned line ranges to begin and end on real symbol
    boundaries, so that I never read half a function.
14. As a developer, I want a chunk returned whole when no symbol span clearly
    matches, so that narrowing never hides the thing I was looking for.
15. As a developer, I want `codebase-retrieval` and `file-retrieval` to behave
    identically in output shape regardless of which reranker ran, so that
    switching providers does not change how I read results.
16. As a developer, I want the reranker to judge my query without the
    `kind:` / `lang:` / `path:` filter prefixes mixed into it, so that the
    relevance judgment is about what I asked, not about my filter syntax.

**Degrading**

17. As the owner, I want retrieval to keep working when my TypeSafe key is
    missing, so that an unconfigured provider degrades instead of failing.
18. As the owner, I want results to fall back to raw similarity order when the
    reranker is unavailable, so that I still get answers.
19. As the owner, I want to be told *why* reranking was skipped, so that silent
    degradation is not mistaken for normal operation.
20. As the owner, I want the engine never to silently substitute my other
    provider when Jev fails, so that my benchmark numbers mean what they say.
21. As the owner, I want rate-limited requests retried with backoff across my
    configured keys, so that transient limits do not surface as failures.
22. As the owner, I want concurrent requests bounded, so that one query cannot
    exhaust my per-minute quota.

**Measuring**

23. As the owner, I want a single command that runs retrieval with reranking on
    and reports recall, so that the stage I am replacing is finally measurable.
24. As the owner, I want that benchmark to compare two rerankers side by side
    with deltas, so that a decision does not rest on an impression.
25. As the owner, I want the benchmark to record the current reranker's numbers
    before Jev exists, so that there is a real baseline rather than a
    reconstructed one.
26. As the owner, I want per-stage timing including the rerank stage, so that a
    quality win that costs latency is visible rather than assumed away.
27. As the owner, I want token usage reported per query, so that I can confirm
    the cost estimate against reality.
28. As the owner, I want the raw probability for every question stored, so that
    I can retune thresholds later without paying for inference again.

**Not breaking things**

29. As the owner, I want repo chat unchanged, so that adopting a reranker does
    not cost me my conversational interface.
30. As the owner, I want agentic RAG unchanged, so that the tool-calling
    retrieval mode keeps working.
31. As the owner, I want embedding configuration untouched, so that adopting a
    reranker never triggers a re-index.
32. As the owner, I want the existing reranker to remain fully functional and
    selectable, so that I can switch back at any time.
33. As a maintainer, I want the refactor that introduces the reranker seam to
    pass the existing test suite with no test edits, so that "no behaviour
    change" is demonstrated rather than claimed.
34. As a maintainer, I want the fork to stay mergeable with upstream, so that
    future upstream features remain pullable.

## Implementation Decisions

**A reranker seam above the LLM client.** A `Reranker` abstraction is introduced
in the query layer, with two implementations: the existing LLM-driven reranker,
and a new Jev-driven one. The query engine and both MCP retrieval tools dispatch
through it. This is deliberately *not* a new provider arm inside the existing
LLM client abstraction: that interface is text-in/text-out across its three
methods, none of which Jev can implement, so an adapter there would have to
reverse-parse a rendered prompt and fabricate a reply. The agentic-RAG backend
abstraction already demonstrates this pattern in the same module and is left
untouched.

**Both implementations return the same output type.** The reranker yields
reordered indices plus optional per-chunk line selections — exactly the shape
the engine already consumes. Result formatting and MCP output assembly are
therefore unmodified, and that invariance is the justification for the seam
sitting where it does.

**Candidate symbol spans are passed in, not looked up.** The query engine
resolves the symbols overlapping each *merged* chunk and passes them to the
reranker. (Graph expansion resolves symbols for base chunks, but merging
reshapes those ranges, so the lookup runs again after the merge; it runs only
when the selected reranker narrows by span.) Each span is clipped to the chunk,
so its edges are symbol boundaries or the chunk's own edges, and a symbol
covering the whole chunk is not offered. The Jev reranker therefore has no
storage dependency, which keeps its tests to a single seam. The existing LLM
reranker ignores the parameter.

**One request per candidate chunk.** Requests fan out concurrently under a
bounded semaphore. Batching all candidates into one request costs identical
input tokens and 30× fewer requests, but every judgment would then see every
other candidate; fan-out is chosen for judgment isolation and the batched mode
is designed for as a strategy on the abstraction, to be built only if rate
limits bite.

**Question design.** Each request carries a relevance question that drives
ranking, plus one question per overlapping symbol span. Questions over shared
state cost no additional state tokens, so the span questions are effectively
free. All are Noul (yes/no probability) questions: the ranking sort is
`descending probability`, which needs no authored rubric. The API contract,
which encodes decisions prose cannot:

```json
{
  "model": "<configured model>",
  "state": {
    "query": "<filter-stripped query text>",
    "file_path": "...", "symbol_name": "...", "symbol_kind": "...",
    "code": "<line-numbered chunk content>"
  },
  "questions": {
    "relevance": { "type": "noul", "instructions": "...", "criteria": { "true": "...", "false": "..." } },
    "span_<n>":  { "type": "noul", "instructions": "...", "criteria": { "true": "...", "false": "..." } }
  }
}
```

The response returns one answer per question key, each carrying a probability,
plus input and output token counts.

**Ranking and narrowing.** Candidates sort by descending relevance probability.
Spans whose probability reaches the threshold (0.5) become the chunk's line
selections, a selected span nested in another folded into the outer one; if
none reach it, or the chunk is under the prune floor, the chunk is emitted
whole. The existing range-padding and
range-sanitising helpers remain in service of the LLM reranker but are not used
by the Jev path, because symbol spans are valid by construction.

**Query text is cleaned before judging.** The reranker currently receives the
raw query string with field-filter prefixes still embedded, while the embedder
receives the stripped version. The stripped text is plumbed through to the
reranker. The MCP layer, which folds structured filter parameters into that
string as prefixes, is accounted for by the same change.

**Configuration.** The settings schema version increments by one. Added: a
rerank provider selector, a TypeSafe API key list held separately from existing
provider keys, and an optional base URL override for the Jev endpoint. The
existing provider field retains its current meaning for chat. On migration, the
rerank provider defaults to whatever the install's existing provider value is —
not to the packaged default — so a custom-endpoint configuration survives.

**Resilience reuses existing patterns.** Key rotation and the two-pass
rate-limit retry are taken from the existing LLM client rather than reinvented.
Transport failure, missing keys, and exhausted retries all resolve to the
existing skip path: results return in similarity order with a populated skip
reason. Falling back to the other provider is explicitly rejected — it would
make both keys permanently required and would corrupt the benchmark by serving
an unknown fraction of queries from the model under comparison.

**Settings UI.** The rerank provider control is relabelled and gains a fourth
option. A TypeSafe key list mirrors the existing key widget. The model
placeholder and the speed hint become provider-aware, with translations in all
three shipped languages.

**Benchmark.** The existing chunk benchmark is extended to run the reranked
retrieval path and to A/B two rerankers, reusing its recall-at-k metrics and
its delta-gate machinery. Its ground truth is derived from the repo under test
(a doc comment is the query, the symbol it documents the expected answer)
rather than the old hand-written C++ cases, which needed a checkout nobody here
has; decided on #7. The query benchmark already
emits per-stage timing including the rerank stage; those figures are recorded
rather than discarded.

**Sequencing.** Three slices, in a forced order: the seam extraction first
(pure refactor); then the benchmark, which must capture the existing reranker's
baseline while it is still the only reranker and which requires the seam to A/B
against; then the Jev implementation.

## Testing Decisions

**What makes a good test here.** Tests assert externally observable behaviour:
the bytes sent on the wire, the ordering and ranges that come back, the
configuration that survives a round trip. They do not assert on internal call
sequences, private helpers, or the shape of intermediate structures. A test that
would fail on a rename but pass on a wrong ranking is not worth writing.

**One new seam: the HTTP wire.** The Jev reranker takes an injectable base URL.
Tests stand up a mock HTTP server, point the reranker at it, capture the raw
request body, and return canned answers. This single seam exercises request
construction, state and question serialization, response parsing, probability
sorting, span selection, whole-chunk fallback, skip-on-failure, and rate-limit
retry — all against real serialization and a real HTTP round trip.

*Prior art:* the integration suite already does exactly this for the
OpenAI-compatible path (`chat_history_reaches_provider_on_wire` and its
neighbour), standing up a mock server and asserting on the serialized request.
That test is only possible because that client's base URL is injectable; the
Gemini client hardcodes its URLs and consequently has no wire coverage. The new
client is born injectable specifically so it does not repeat that.

Because symbol spans arrive as a parameter rather than a storage lookup, no test
database or indexed fixture repo is needed for span-selection tests.

**Reused seam: configuration round-trip.** The schema migration is covered by
the existing config fixtures in the integration suite, which already assert that
rerank settings persist across a write-read cycle. A fixture for the
custom-endpoint provider is required, not optional: that is the configuration
the owner actually runs, and a migration verified only against the packaged
default would strand it.

**Reused seam: the whole suite.** The seam-extraction slice is verified by the
existing tests passing with **zero test edits**. Any test change during that
slice is evidence the refactor was not behaviour-preserving.

**Not tested automatically.** The settings page. This repo has no frontend test
harness and this spec does not introduce one. Verification is a manual page
load: the provider appears, the key list accepts and persists a key, no console
errors, and all three languages render.

**Benchmark is tooling, not a test.** It is not wired into the test suite and
does not gate the build. It gates the decision to change the default, which is
a separate event from shipping this work.

## Out of Scope

- **Repo chat and agentic RAG.** Both stay on the existing chat model
  permanently. Jev cannot generate text, stream, or call tools; this is a
  property of the model, not a backlog item.
- **Embeddings.** A separate provider axis with its own identity and
  invalidation machinery. Untouched, and nothing here triggers a re-index.
- **Flipping the default.** Shipped opt-in. Changing the default is gated on
  benchmark results and is a separate decision with its own evidence.
- **The second relevance dimension** (distinguishing a definition from a call
  site or a test). Nearly free to add, but it needs a combination policy and the
  single-question baseline has to be observed first.
- **Batched request mode.** Designed for as a strategy on the abstraction;
  built only if rate limits prove to be a problem in practice.
- **Adding a response schema to the existing LLM reranker**, which would delete
  its repair loop. Worth doing regardless of how this resolves, but not here.
- **Removing the existing reranker.** It stays fully functional and selectable.
- **Contributing any of this upstream.**

## Further Notes

**This rests on a bet that the benchmark exists to settle.** The vendor's
published reranking result — top-1 accuracy roughly tripling — was measured on
legal prose, not source code, and the vendor's own guidance says such figures
are examples to evaluate rather than universal rules. If the bet fails, the
implementation is sunk cost and the existing reranker remains the default.

**Narrowing is a second, separable bet.** That symbol spans make better
retrieval units than model-chosen line ranges is recorded in the PRD as
judgment, not evidence. It can be reverted independently: the Jev path could
rank while leaving chunks whole, without abandoning the ranking win.

**The request-rate profile changes by a factor of thirty.** One query moves from
a single call to roughly thirty. Against the vendor's per-minute limit this
allows on the order of forty queries a minute, which is generous for a tool
bound to loopback and serving one developer, and inadequate for shared or
multi-tenant deployment. The batched mode is the designed escape hatch.

**Costs are effectively nil.** Roughly fifteen thousand input tokens per
reranked query at the vendor's input-token price puts a query well under a tenth
of a cent, which is why a modest quality win is worth shipping rather than
demanding a dramatic one.

**Fork mergeability is a live constraint.** The change adds alongside the
existing provider paths rather than replacing them, and the settings-page edits
are small and localised, which keeps future upstream merges tractable.
