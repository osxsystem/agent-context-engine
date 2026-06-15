# CLAUDE.md

Guidance for AI agents working in this repository. See `@README.md` for the
user-facing feature list, supported-language table, and the end-to-end flow
diagram; this file captures only what an agent needs every session and does not
duplicate the README.

## Project summary

`context-engine-rs` (published as `vibervn-context-engine`, Rust **edition
2024**) is a local semantic code-search / context engine for AI agents. It
indexes a codebase, extracts symbols and call-graph edges across 22 languages
with tree-sitter, embeds code chunks via **Voyage AI** into an embedded
**SurrealDB** (RocksDB-backed, one datastore per repo), and serves results over
an HTTP API + Web UI and an **MCP server** exposing two tools:
`codebase-retrieval` and `file-retrieval`.

## Build / test commands

```bash
cargo build                 # builds the default binary (the server)
cargo test                  # full suite (lib unit tests + integration tests)
cargo run                   # runs the server (default-run is pinned, see below)
cargo run --bin chunk_bench # runs the benchmark harness instead
```

Toolchain: edition 2024 requires **Rust 1.85+** (verified on 1.96). A C compiler
(`cc`/clang) is required to build the vendored tree-sitter grammar scanners.
Building emits ~21 benign C `unused parameter` warnings from those grammars
(e.g. `tree-sitter-liquid`); these are not from our Rust code.

### Two-binary gotcha

There are **two binaries**: the server (`src/main.rs` → `context-engine-rs`) and
a benchmark harness (`src/bin/chunk_bench`). `Cargo.toml` sets
`default-run = "context-engine-rs"`, so `cargo run` / `cargo watch -x run`
unambiguously launch the server. Without that pin, cargo errors with "could not
determine which binary to run". To run the bench harness, pass
`--bin chunk_bench` explicitly.

### Test baseline (macOS / non-Windows)

- Lib unit tests: **375 pass, 7 fail** by design. The 7 failures are all
  `mcp::tests::file_retrieval_db_key_*`. They assert backslash-separated Windows
  paths but are **not** `#[cfg(windows)]`-gated, while `build_db_key`
  (`src/mcp.rs`) converts `\`→`/` on non-Windows via `Path::join`. They pass
  only on Windows. Treat them as a known platform-specific baseline, **not** a
  regression you introduced.
- Integration tests (`tests/integration.rs`, `tests/repro_notepad.rs`): **11
  pass, 3 ignored**. The 3 ignored tests in `repro_notepad.rs` are manual
  diagnostic harnesses pinned to Windows paths (`D:/projects/Cpp/notepad-ade`)
  and a real on-disk `~/.vibervn` index; run them with `-- --ignored`.

## Module map (`src/`)

| Path | Role |
|------|------|
| `main.rs` | Binary entry: CLI parsing, boot-time path/env resolution, RocksDB memory bounds, starts IndexEngine + Axum server. |
| `config.rs` | `Settings` schema, versioned migrations, atomic `settings.json` read/write. |
| `server.rs` | Axum router, `AppState`, all HTTP/SSE routes, mounts the `/mcp` service. |
| `mcp.rs` | MCP server: `McpHandler` / `RepoMcpHandler`, the `codebase-retrieval` & `file-retrieval` tools, `run_codebase_retrieval` / `run_file_retrieval`. |
| `indexing/` | Index pipeline (`pipeline.rs`), file walker, watcher, change tracker, import resolver, framework extractors, event bus. |
| `parsing/` | tree-sitter symbol extraction (`symbols.rs`), chunking (`chunker.rs`), call-edge relations (`relations.rs`), language detection (`mod.rs`). |
| `embedding/` | Voyage AI HTTP client (`voyage.rs`) + on-disk content-addressed cache (`cache.rs`). |
| `vector/` | In-memory sharded vector index (`sharded.rs`) — the hot cosine-search path; loaded from SurrealDB at boot. |
| `query/` | Query engine (`engine.rs`), field filters, graph expansion, chunk merging, LLM reranker. |
| `store/` | SurrealDB schema (`schema.rs`), CRUD ops (`ops.rs`), repo-path normalization, per-repo DB handle map. |
| `llm/` | Optional rerank LLM clients: Google Gemini (`google.rs`) and OpenAI-compatible (`openai.rs`). |
| `defender.rs` | Windows Defender exclusion helper (RocksDB perf). |
| `assets/index.html` | Bundled Web UI. |

## Runtime requirement: API keys

- **Voyage API key is required** for any indexing or query — embeddings go
  through Voyage AI. With no embedding key configured, indexing/query paths
  return an error (e.g. `run_file_retrieval` returns "no embedding API keys
  configured"). The server still boots and serves the UI without one.
- An **LLM rerank key is optional** (Google Gemini or OpenAI-compatible).
  Reranking is a quality-boosting step that can be disabled.
- Never write API keys into tracked files. Keys live in `settings.json` (see
  below) or are supplied at runtime; `settings.json` is written `0o600` on Unix.

## Runtime configuration

(From `src/main.rs` and `src/config.rs` — documented without running the server.)

### Where settings live

`settings.json` lives at a **fixed** location, independent of `data_dir`:

```
~/.vibervn/context-engine/settings.json
```

(`config_path` in `config.rs`.) Its location is fixed on purpose — the
`data_dir` field lives *inside* settings.json, so deriving the file's location
from that field would be circular. The schema is versioned
(`CURRENT_VERSION = 7`) with forward migrations run on load; a file written by a
newer binary is refused with `VersionTooNew`.

Key `Settings` fields: `repos` (absolute indexed paths), `embedding` (provider
`voyage`, model `voyage-4-lite`, `api_keys`, `embed_concurrency`,
`voyage_base_url`), `llm` (rerank provider/model/keys, `agentic_rag*`,
`openai_base_url`), `enabled_mcp_tools`, `custom_extensions`,
`index_ignore_filenames` (defaults to `CLAUDE.md`, `AGENTS.md` — so this very
file is excluded from indexing), `vector_resident_cap_mb` (default 2048),
`data_dir`, `embeddings_dir`.

### Environment variables / CLI flags

| Flag | Env var | Default |
|------|---------|---------|
| `--port` | `CONTEXT_ENGINE_PORT` | `6699` |
| `--bind` | `CONTEXT_ENGINE_BIND` | `127.0.0.1` |
| `--data-dir` | `CONTEXT_ENGINE_DATA_DIR` | `~/.vibervn/context-engine` |
| `--embeddings-dir` | `CONTEXT_ENGINE_EMBEDDINGS_DIR` | `~/.vibervn/context-engine/embeddings` |

Also honored: `RUST_LOG` (tracing filter; defaults to
`context_engine_rs=info,warn`) and `SURREAL_ROCKSDB_*` (RocksDB memory bounds —
`main.rs` pins small repo-count-stable defaults unless already set).

**Boot precedence** for `data_dir` / `embeddings_dir`: CLI flag > env var >
`Settings.<field>` > builtin default. Resolved **once** at boot and frozen — a
`PUT /api/config` that changes these only takes effect on the next launch
(keeps open RocksDB handles and warmed vector shards consistent). RocksDB lives
at `<data_dir>/rocksdb/` and takes an exclusive per-directory lock (point each
concurrent instance at its own `data_dir`); the content-addressed embedding
cache is concurrency-safe and meant to be **shared** (hence anchored to home by
default).

### Default server address

`http://127.0.0.1:6699` — Web UI at `/`, MCP endpoint at `/mcp`
(per-repo MCP at `/mcp-repo/:repo_id`). Boot fails fast (exit 2) if it cannot
determine the home dir, load settings, create `data_dir`, or bind the address.

## Architecture (core flow)

Boot → HTTP routes → indexing → query → MCP exposure, tracing the five core files:

1. **`src/main.rs`** — `#[tokio::main]` entry. Calls `set_rocksdb_memory_bounds()`
   *before* any datastore opens, parses the `Cli` (clap), resolves
   `port`/`bind`/`data_dir`/`embeddings_dir` with the documented precedence,
   loads `Settings` via `ensure_dir_and_load`, wraps them in an
   `Arc<RwLock<Settings>>`, starts `IndexEngine::start(...)` (spawns per-repo
   watchers, shares the `repo_dbs` map), builds the router with
   `server::build_router(...)`, and serves it with `axum::serve`.

2. **`src/server.rs`** — `build_router` constructs `AppState` (home/data/
   embeddings dirs, `Arc<IndexEngine>`, per-repo `Surreal<Db>` map, live
   `Settings` handle, per-repo MCP services) and registers all routes:
   `/api/config` (GET/PUT), per-repo index/rebuild/cancel/status/files/graph/
   chunks/SSE-events endpoints, `/api/query`, `/api/mcp-tool*`, plus the Web UI
   at `/`. It mounts the streamable-HTTP MCP service at `/mcp` (factory closure
   builds a fresh `McpHandler` per session, honoring `enabled_mcp_tools` and
   DNS-rebinding protection for non-loopback binds) and `/mcp-repo/:repo_id`.

3. **`src/indexing/pipeline.rs`** — `IndexPipeline` (builder:
   `new_with_concurrency`, `with_extra_extensions`, `with_ignore_filenames`,
   `with_ignore_paths`). `IndexPipeline::run(...)` drives indexing against a
   shared `Surreal<Db>`: walk + detect changed files, parse (tree-sitter
   symbols/chunks/raw edges) → framework extraction → embed chunks (Voyage +
   cache) → store chunks/symbols, then **Phase 2** resolves `raw_edge` rows into
   denormalized `calls` rows (import resolution + name matching). It is
   incremental (per-file commit markers), crash-safe (replays Phase 2 from the
   durable `raw_edge` table), cancellable, and bounds memory (edges overflow to
   DB past `MAX_RAM_EDGES`). Returns `IndexPipelineStats`.

4. **`src/query/engine.rs`** — `run_query` / `run_query_with_filters` is the
   query pipeline: parse field filters (`kind:`/`lang:`/`path:`/`name:`) → embed
   the cleaned query (Voyage) → vector search (`index_engine.vector_search`, top
   `2×k` cosine over the in-RAM shards) → repo filter → fetch stored chunk
   content → apply filters → **graph expansion** (BFS callers/callees) → merge +
   dedup adjacent ranges → fetch caller/callee stats → **LLM rerank**
   (single-shot or agentic loop) → format `path#Lstart-end` with numbered lines
   and caller/callee tags. Returns `QueryResult` (`CodeResult[]` + `QueryTiming`
   + `RerankInfo`).

5. **`src/mcp.rs`** — `McpHandler` (global, searches all configured repos) and
   `RepoMcpHandler` (scoped to one repo) implement the `rmcp` `ServerHandler`.
   The `#[tool_router]`/`#[tool]` macros expose `codebase-retrieval` and
   `file-retrieval`; `McpHandler::new` disables routes for tools absent from
   `enabled_mcp_tools`. The tool methods delegate to `run_codebase_retrieval`
   (wraps the `query::engine` pipeline, waits up to `mcp_index_wait_secs` for
   indexing, checks freshness) and `run_file_retrieval` (embeds the request,
   fetches one file's chunks via `build_db_key`, cosine-ranks in memory).
