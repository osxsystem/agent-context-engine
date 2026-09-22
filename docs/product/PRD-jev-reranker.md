# PRD: Adopt Jev as the reranker

**Status:** Drafted, not yet approved
**Repository:** `osxsystem/agent-context-engine` (fork of `nullmastermind/vibervn-context-engine`)
**Version at time of writing:** 1.0.0 (fork line; upstream merged through v0.1.73)
**Author:** Do Viet Hung

---

## 1. Summary

The Context Engine ranks candidate code chunks by asking a general-purpose LLM
for a JSON blob and parsing it back. TypeSafe's **Jev** is a different kind of
model: it answers typed questions with calibrated probabilities, so no parsing
is involved. This document proposes adopting Jev for the reranking stage — and
only that stage — replacing prompt-and-parse ranking with probability sorting,
and replacing model-invented line ranges with selection over real symbol spans.
Adoption is opt-in and becomes the default only if it wins a recall benchmark.

---

## 2. Contacts

| Name | Role | Comment |
| ---- | ---- | ------- |
| Do Viet Hung | Owner / Maintainer | Sole decision-maker. Personal fork, personal business. |
| TypeSafe (`console.typesafe.ai`) | Model vendor | API key already held. No relationship beyond metered API access; no SLA, no support contract. |
| `nullmastermind/vibervn-context-engine` (upstream) | Source project | **Deliberately not consulted.** Divergence is intentional; this change is not to be pushed upstream. |

---

## 3. Background

**What this is about.** Retrieval in this engine runs: embed → vector search →
graph expansion → merge → **rerank** → format. The rerank stage
(`src/query/reranker.rs:47`) sends up to 30 candidate chunks to a chat LLM in a
single call and asks for `{"ranked_indices": [{"chunk_index": i, "lines":
[[start, end]]}]}`. That output does two jobs at once: it *orders* the chunks
and it *narrows* each one to the lines worth showing.

The whole arrangement is held together by prompting, not by types. The Gemini
path sends `responseMimeType: "application/json"` but **never sends a
`responseSchema`** (`src/llm/google.rs:209`) — the required shape is described
in prose. That is why the code carries a repair loop for malformed `lines`
shapes and `MAX_FORMAT_RETRIES` (`src/query/reranker.rs:412`), and why the model
is trusted to invent line numbers that become the `path#Lstart-end` headers
users actually read.

**Why now.** Two things changed. First, the owner obtained a TypeSafe API key,
making Jev 1.13 usable. Second, Jev is a *System One* model — it returns Choice,
Score, or Noul values with probabilities instead of generating text — which
makes it structurally suited to the ranking half of this job and structurally
incapable of the two other model call sites in this codebase. That asymmetry is
what makes the change tractable: the blast radius is one stage, not the engine.

The reranker is also the one stage with no measurement. `bench-query --rerank`
is off by default — *"which is what the bench uses"*
(`src/bin/bench_query/main.rs:68-71`) — and `chunk_bench --ab` compares chunkers.
So the component being replaced has never been benchmarked in this repo.

---

## 4. Objective

Improve retrieval quality at the rerank stage by replacing a prompted
text model with a model that answers the actual question — *is this chunk
relevant?* — as a calibrated probability, and by grounding returned line ranges
in real symbol boundaries instead of model guesses.

This benefits the owner directly: better first results from
`codebase-retrieval`, which is the tool every MCP client in the owner's
workflow depends on. It costs roughly **$0.0006 per query**, so the change is
effectively free to run. It aligns with what this fork is for — a private,
self-hosted engine on the owner's own keys, tuned for the owner's own repos.

It is explicitly **not** a consolidation play. Gemini remains required.

### Key Results

Measured on the owner's corpus with the extended rerank benchmark (KR5),
comparing Jev against the currently-configured reranker.

| # | Result | Target | Baseline |
| - | ------ | ------ | -------- |
| KR1 | `recall_at_1` | **No regression** vs current reranker | ⚠ Unmeasured — no rerank benchmark exists. KR5 establishes it. |
| KR2 | `recall_at_5` or `recall_at_10` | **Measurable improvement** (at least one moves up) | ⚠ Unmeasured, per KR1. |
| KR3 | `rerank_ms`, p50 | **≤ current p50** | ⚠ Unmeasured. Emitted today by `bench_query/main.rs:418` but never recorded. |
| KR4 | Cost per reranked query | **≤ $0.001** | $0 marginal visibility today (bundled into Gemini spend). Computed from Jev's `usage.input_tokens`. |
| KR5 | Benchmark covering the reranked retrieval path | **Exists, one command, A/B two rerankers** | **Does not exist.** |
| KR6 | `cargo test` | **green** | green (must not regress) |

KR1 and KR2 together are the gate: Jev becomes the default only if recall@1
holds and recall@5 or recall@10 improves. KR3 is a hard gate — a quality win
that makes every query slower is not accepted silently.

**Deadline:** none committed. This is owner-paced work with no external date.

---

## 5. Market Segment(s)

The same segment of one as the rest of this fork: **the owner of a self-hosted
personal Context Engine instance who supplies their own provider keys**, running
local semantic code search over personal repositories.

The job this addresses is narrower than the fork's overall job: *get the right
code into an agent's context on the first try*. The consumers are the MCP
clients (Claude Code and similar) that call `codebase-retrieval` and
`file-retrieval`, where a bad first result costs the agent a wasted turn.

Two constraints shape the work:

- **The fork must stay mergeable with upstream.** Upstream keeps shipping
  features worth pulling, so the change must sit alongside the existing
  provider paths rather than replacing them.
- **The deployment is local.** The engine binds `127.0.0.1:6699` by default and
  serves one developer, which is what makes a 30-requests-per-query design
  acceptable (see A2).

Explicitly **out of segment:** anyone running this engine as shared or
multi-tenant infrastructure. See A2 — the request-rate profile changes by 30×
and the design assumptions stop holding.

---

## 6. Value Proposition(s)

**Jobs addressed**

- Rank retrieved code by actual relevance, not by an LLM's willingness to emit
  well-formed JSON.
- Return line ranges a reader can trust to be a whole unit of code.
- Know whether the reranker is helping at all, which is currently unknowable.

**Gains**

- Ranking driven by a calibrated probability, which is sortable, thresholdable,
  and inspectable — the raw scores are stored, so tuning later doesn't require
  re-running inference.
- Returned spans align to real symbol boundaries rather than a model's guess.
- Reranking cost drops to roughly $0.0006/query.
- A rerank benchmark exists, permanently, for any future reranker change.

**Pains avoided**

- A malformed-JSON repair loop that exists only because the output contract is
  prose.
- `path#Lstart-end` headers that can cut a function in half — today the only
  safeguards are a ±2-line pad, a clamp to chunk bounds, and a 16-line floor.

**Why better than the alternatives.** Three were considered:

- *Keep Gemini, add `responseSchema`* — a much smaller change that would delete
  the repair loop. Rejected as insufficient: it makes the JSON reliable but
  leaves ranking as a side effect of text generation, and still lets the model
  invent line numbers. Worth doing anyway if Jev fails the gate.
- *Reach Jev through the existing "Custom" OpenAI-compatible provider* — the
  owner already runs `provider: "custom"` against a local proxy. Not possible:
  Jev's endpoint is `POST /v1/systemone` taking state and questions, not
  `/v1/chat/completions` taking messages. There is no compatibility shim.
- *Add Jev as a provider arm inside `LlmClient`* — cheapest diff. Rejected in
  review: `LlmClient` (`src/llm/mod.rs:60`) is a text-in/text-out interface
  whose three methods Jev cannot implement, so the adapter would have to
  reverse-parse a rendered prompt string and fabricate a JSON reply. See 7.3.

---

## 7. Solution

### 7.1 UX

**Before.** Settings → *LLM Keys* shows a Provider dropdown
(`src/assets/index.html:586`) offering Google / OpenAI / Custom, which governs
reranking *and* is the fallback for repo chat. Alongside it sit rerank-specific
controls: Min prune lines, Use structured output, Agentic RAG.

**After.** The same dropdown is relabelled **"Rerank provider"** — an honest
name for what it always was, since every control in that panel is rerank-shaped
— and gains a fourth option, **TypeSafe (Jev)**. Selecting it reveals a
TypeSafe API keys list mirroring the existing widget, and the model field
defaults to `jev-latest`. Repo chat keeps its own provider selector, which
already exists (`index.html:5364`).

Query results look the same. The visible difference is that
`path#Lstart-end` ranges land on symbol boundaries.

**No flow is added or relocated.** Existing installs see no change: the config
migration carries the current provider forward, so the panel behaves exactly as
before until the owner opts in.

### 7.2 Key Features

Three slices, in order. The sequence is forced: the benchmark must capture a
Gemini baseline *before* Jev exists, and A/B-ing two rerankers requires the
trait seam, so the benchmark sits between them.

**Slice 1 — `Reranker` trait extraction.** Pure refactor, zero behaviour
change, verifiable with no API key and no network. Today's `rerank()` becomes
`LlmReranker::rerank`; call sites at `src/query/engine.rs:352-410`,
`src/mcp.rs:1115-1127`, and `src/engine_ops.rs:190` dispatch through
`dyn Reranker`. `AgenticBackend` (`reranker.rs:424`) is untouched.

**Slice 2 — rerank benchmark (KR5).** Extend the `chunk_bench` harness to run
the reranked retrieval path and A/B two rerankers on `recall_at_1/5/10` and
`rerank_ms`, reusing the existing ground truth and delta-gate machinery
(`src/bin/chunk_bench/main.rs:82-84`, `:253-255`). Record the Gemini baseline.

**Slice 3 — Jev reranker.**

| Area | Change |
| ---- | ------ |
| New | `src/query/rerank/jev.rs` — one request per candidate, semaphore-bounded, key rotation and 429 two-pass retry copied from `llm/mod.rs:165` |
| Spans | `src/query/graph_expand.rs:223` — `query_overlapping_symbols` / `SymbolRow` become `pub(crate)`; the engine keeps spans it already computes and currently discards |
| Query text | `src/query/engine.rs:211-226, :410` — pass `clean_query`; today rerank receives the raw string with `kind:` / `lang:` / `path:` filters still in it |
| Config | `src/config.rs:12` — `CURRENT_VERSION` 13→14; add `rerank_provider` (defaults to existing `provider` on migrate), `jev_api_keys`, `jev_base_url` |
| UI | `index.html:586` relabel, `:591` add option, key list, `:624`/`:629`/`:1346` model placeholder and `llm.rerankModelHint` in en/vi/zh |
| Tests | axum wire test mirroring `tests/integration.rs:1056`; migration fixture covering `provider: "custom"` |

**Deliberately unchanged.** Repo chat (`src/chat.rs:885`) and agentic RAG
(`src/query/reranker.rs:508`) stay on the existing LLM path — Jev cannot
generate text or call tools. `RANGE_PAD` and `sanitize_ranges` stay for
`LlmReranker`; `JevReranker` simply never needs them.

### 7.3 Technology

Three technical facts shape the product:

- **Jev is not an LLM and cannot substitute for one.** It returns Choice, Score,
  or Noul values — no text generation, no tool calls, no streaming. TypeSafe's
  own documentation states it is *"not a drop-in replacement for the LLM behind
  Claude Code, Cursor, Copilot."* This is why the change covers one of three
  model call sites and why "adopt Jev as the main model" is not achievable.
- **The seam goes above `LlmClient`, not inside it.** Both `LlmReranker` and
  `JevReranker` return the same `RerankOutput { reranked_indices,
  line_selections }`, which leaves `engine.rs:423-500` and all MCP output
  formatting untouched. That invariance is the justification for the refactor.
- **Ranking and pruning ride in one request.** For each chunk, the request
  carries a relevance Noul plus one Noul per overlapping symbol span. Questions
  over shared state cost no extra state tokens, so the pruning judgments are
  effectively free. Pruning becomes *selection among existing candidate spans*
  rather than generation of new ones.

Operating envelope: 64k tokens per request (32k for state plus longest
question), 1,200 requests/minute, $0.042 per million input tokens, text only.

### 7.4 Assumptions

| # | Assumption | Basis | If wrong |
| - | ---------- | ----- | -------- |
| A1 | Jev's relevance judgment transfers from prose to source code. | TypeSafe's rerank cookbook reports top-1 accuracy 5%→18% — on court opinions, not code. The docs warn cookbook figures are *"examples to evaluate, not universal rules."* | The KR1/KR2 gate fails, Jev stays opt-in, and the implementation cost is sunk. This is the central bet and the benchmark exists to settle it. |
| A2 | 30 requests per query stays inside 1,200 req/min for single-developer local use. | The engine binds `127.0.0.1:6699` and serves one person; 1,200 ÷ 30 = 40 queries/min, well beyond human query rate. | Sustained 429s under concurrent MCP clients or `--bind 0.0.0.0`. Mitigated by design: batched mode (all chunks in one request, identical token cost) is a knob on the trait, not a rewrite. |
| A3 | `query_overlapping_symbols` returns useful spans for most chunks. | The query already runs per base chunk and is backed by `idx_symbol_file`. Chunks come from cAST split-then-merge, so some straddle symbol boundaries and some contain several symbols. | Chunks with no clean span fall back to whole-chunk emission, growing payloads against `MAX_TOOL_OUTPUT_CHARS = 48_000`. Measurable as the fallback rate. |
| A4 | Symbol spans are better retrieval units than model-chosen line ranges. | Judgment, not evidence. Unproven — this is the pruning half of the bet, separable from A1 and observable via the harness's IoU metric. | Recall holds but the returned spans are less useful. Reverting pruning alone is possible without abandoning Jev ranking. |
| A5 | A candidate chunk always fits Jev's 32k state budget. | `MAX_CHUNK_NONWS = 1500` non-whitespace chars per chunk (`chunker.rs:26`), merged ranges capped at 60 lines (`merger.rs:71`). | Oversized leaves are emitted whole by the chunker and could in principle exceed it; needs a guard and a truncation path. |
| A6 | The v13→v14 migration is safe against the owner's live config, which runs `provider: "custom"` with a local endpoint — not the `"google"` default. | Observed directly in the running settings UI. | Migration tested only against the default would strand the owner's working configuration. A `custom` fixture is required, not optional. |
| A7 | TypeSafe remains available and its rate limits stay workable. | Vendor documentation notes limits are *"adjusting dynamically"* due to demand. | Reranking degrades to cosine order with a visible `skip_reason`; retrieval keeps working, less well. No silent fallback to Gemini, by design. |

---

## 8. Release

**First version.** Slices 1–3 from section 7.2, landed on the fork's `master`,
with `rerank_provider` shipping **opt-in**. Migration carries every existing
install forward unchanged. No flag beyond the provider selector itself, no
staged audience.

Verification gate before the work is called done:

1. `cargo test` passes, including the new wire test and the `custom`-provider
   migration fixture.
2. The settings page loads with zero console errors and the new provider is
   selectable in all three languages.
3. The rerank benchmark runs end to end and reports both rerankers side by side.
4. `codebase-retrieval` and `file-retrieval` both return results with Jev
   selected, and degrade to cosine order with a visible `skip_reason` when the
   key is absent or rate-limited.

**Second version — flipping the default.** Gated strictly on KR1, KR2 and KR3.
If recall@1 regresses, or neither recall@5 nor recall@10 improves, or p50
`rerank_ms` gets worse, the default does not move and Jev remains an option.
No timeframe; this happens when the numbers are in.

**Deliberately deferred, with no committed timeframe.**

- The second Noul (*"is this the definition rather than a call site or test?"*),
  which would address the classic code-retrieval failure mode. Nearly free to
  add, since it shares the chunk's state tokens — but it needs a combination
  policy, and the single-Noul baseline has to be observed first.
- Batched request mode (all chunks in one request). Designed for as a knob;
  built only if A2 breaks.
- Sending `responseSchema` on the Gemini path, which would delete the repair
  loop for `LlmReranker` regardless of how the Jev bet resolves.

**Not planned.** Any Jev involvement in repo chat or agentic RAG — the model
cannot generate text or call tools, so this is a permanent boundary, not a
backlog item. Nor any contribution of this change upstream.

---

## Provenance

This PRD was produced from a five-round design review covering: what Jev can
and cannot replace, where the integration seam belongs, how pruning survives a
model that cannot emit line ranges, request batching versus fan-out, the
configuration and UI surface, and the measurement gate.

**Correction log.** Four claims made during the review were wrong and were
corrected before they reached this document:

1. *"The LLM is only the reranker."* A codebase audit found three model call
   sites — rerank, agentic RAG, and repo chat. Jev can serve one. This corrected
   the scope from "replace the main model" to "replace the reranker," which is
   the premise the rest of the document rests on.
2. *"Prune by snapping to the enclosing tree-sitter symbol."* Chunks are cAST
   split-then-merge, not symbol-aligned, so snapping would frequently *expand* a
   chunk rather than narrow it. Replaced with selection among overlapping spans.
3. *"The web UI source is not in this repo."* False. `src/assets/index.html` is
   hand-written source, not a build artifact — the initial search missed its
   contents because the file uses CRLF line endings. This removed a "permanent
   gap, config-file only" caveat that would have shipped in the plan.
4. *Fan-out was recommended before the request-rate arithmetic was done.*
   Batching turns out to cost identical input tokens and 30× fewer requests, so
   fan-out buys only judgment isolation. Fan-out was kept, but as a measured
   trade with a documented fallback (A2), not as a default assumption.

**Next step:** `/to-spec` to turn this into buildable work. Specs deriving from
this document should reference `docs/product/PRD-jev-reranker.md`.
