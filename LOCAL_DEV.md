# Local Development Guide (macOS)

A step-by-step, beginner-friendly guide to build, run, and test
`context-engine-rs` on a local macOS machine. Every command is copy-pasteable.
Follow the sections **in order** the first time; later you'll only need
sections 4–9.

> Mental model first: this is a long-running **HTTP server** (default
> `http://127.0.0.1:6699`) that indexes folders of code into a local database,
> turns code into embedding vectors via the **Voyage AI** API, and answers
> semantic search queries — over a Web UI, a REST API, and an MCP endpoint.
> To *build and test the code* you need nothing external. To *actually run a
> useful query* you need a free Voyage API key (section 7).

---

## 0. TL;DR (if you just want the commands)

```bash
# prerequisites (one time)
xcode-select --install                              # C compiler (clang)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust, if not installed

# in the repo root:
cargo build            # compile (debug)
cargo test             # run tests — expect 7 known Windows-only failures (see §3)
cargo run              # start the server on http://127.0.0.1:6699
```

Open <http://127.0.0.1:6699> in a browser, paste a Voyage key + add a repo
folder in **Settings**, wait for indexing, then search. Details below.

---

## 1. Prerequisites & verification

You need three things: a C compiler, the Rust toolchain (1.85+, because this
crate is **edition 2024**), and git.

### 1a. C compiler (clang)

The project compiles ~22 vendored tree-sitter grammars written in C, so a C
compiler must be on `PATH`. On macOS this comes from the Xcode Command Line
Tools:

```bash
xcode-select --install      # opens a GUI installer; skip if already installed
cc --version                # must print "Apple clang version ..."
```

### 1b. Rust toolchain

Install via [rustup](https://rustup.rs) (the official installer) if you don't
have it:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# then restart your terminal, or:
source "$HOME/.cargo/env"
```

Verify versions — **both must be ≥ 1.85** (edition 2024 requirement):

```bash
rustc --version     # e.g. rustc 1.96.0
cargo --version     # e.g. cargo 1.96.0
```

If yours is older, update (rustup keeps it in your home dir, no sudo):

```bash
rustup update stable
```

### 1c. git

```bash
git --version       # macOS ships git via the Xcode CLT above
```

### Prerequisite checklist

| Tool | Command | Expected |
|------|---------|----------|
| C compiler | `cc --version` | `Apple clang version 21.x` |
| Rust compiler | `rustc --version` | `≥ 1.85` |
| Cargo | `cargo --version` | `≥ 1.85` |
| git | `git --version` | any recent version |

---

## 2. Get the code & enter the repo

If you already have the folder, just `cd` into it. Otherwise clone it, then:

```bash
cd /path/to/agent-context-engine    # the folder containing Cargo.toml
ls Cargo.toml src/main.rs           # sanity check you're in the right place
```

All `cargo` commands below are run from this directory.

---

## 3. Build & test (no external services needed)

### 3a. First build (debug)

```bash
cargo build
```

What to expect the **first** time:

- It downloads & compiles ~450 crates **plus** the C tree-sitter grammars. This
  can take a few minutes on first run — that's normal. Subsequent builds are
  incremental and fast.
- You will see ~21 **C compiler warnings** like
  `tree-sitter-liquid: unused parameter 'payload'`. These come from the vendored
  grammar code, **not** from this project. They are harmless — ignore them.
- Success looks like: `Finished \`dev\` profile [unoptimized + debuginfo] target(s)`.

A release (optimized) build — slower to compile, much faster to run, used for
real indexing of large repos:

```bash
cargo build --release        # binaries land in target/release/
```

### 3b. Run the test suite

```bash
cargo test
```

**Read this before you panic at red output.** On macOS the expected baseline is:

- **Library unit tests: 375 pass, 7 fail.**
- **Integration tests: 11 pass, 3 ignored.**

The **7 failures are expected on macOS** and are *not* your fault. They are all
named `mcp::tests::file_retrieval_db_key_*`. They assert Windows-style
backslash paths (`D:\projects\...`) but aren't gated to only run on Windows,
while the real code (`build_db_key` in `src/mcp.rs`) correctly produces
forward-slash paths on macOS/Linux. So they're green only on Windows. Treat
this as a known platform quirk.

The **3 ignored integration tests** (`tests/repro_notepad.rs`) are manual
diagnostic harnesses hard-wired to a Windows path (`D:/projects/Cpp/...`) and a
real on-disk index. They're skipped on purpose.

> ✅ **Your build is healthy if you see `375 passed; 7 failed` for the lib tests
> and `11 passed; 3 ignored` for the integration tests.**

### 3c. Handy test commands

```bash
cargo test config                       # run only tests whose name contains "config"
cargo test --lib                        # only the in-crate unit tests
cargo test --test integration           # only tests/integration.rs
cargo test -- --ignored                 # also run the #[ignore]'d diagnostics (mostly Windows-only)
cargo test -- --nocapture               # show println!/log output from tests
cargo test --release                    # run tests against the optimized build
```

To prove to yourself the 7 failures are *only* the Windows path tests:

```bash
cargo test --lib 2>&1 | grep -E "FAILED|test result"
```

---

## 4. Run the server

```bash
cargo run
```

(`cargo run` is unambiguous here because `Cargo.toml` pins
`default-run = "context-engine-rs"` — see §5 about the second binary.)

You should see logs ending with:

```
Context Engine listening on http://127.0.0.1:6699
```

Now open <http://127.0.0.1:6699> — that's the Web UI. The MCP endpoint is at
`/mcp`. Press `Ctrl-C` in the terminal to stop the server.

### Change port / bind address

Via flags or environment variables (flag wins over env var):

```bash
cargo run -- --port 8080 --bind 127.0.0.1
# or
CONTEXT_ENGINE_PORT=8080 cargo run
```

Useful env vars:

| Variable | Purpose | Default |
|----------|---------|---------|
| `CONTEXT_ENGINE_PORT` | listen port | `6699` |
| `CONTEXT_ENGINE_BIND` | bind address | `127.0.0.1` |
| `CONTEXT_ENGINE_DATA_DIR` | database location | `~/.vibervn/context-engine` |
| `CONTEXT_ENGINE_EMBEDDINGS_DIR` | embedding cache | `~/.vibervn/context-engine/embeddings` |
| `RUST_LOG` | log verbosity | `context_engine_rs=info,warn` |

Example with verbose logs (great for learning what the server does):

```bash
RUST_LOG=context_engine_rs=debug cargo run
```

> Note: `--data-dir`/`--embeddings-dir` are resolved **once at boot**. Changing
> them later in Settings only takes effect on the *next* launch (this protects
> open database handles). Use a different `--data-dir` if you want a throwaway
> sandbox that doesn't touch your real index.

---

## 5. The two-binary gotcha

This crate produces **two** binaries:

1. `context-engine-rs` — the server (`src/main.rs`). This is the default.
2. `chunk_bench` — a chunking benchmark harness (`src/bin/chunk_bench.rs`).

Because `default-run` is set, `cargo run` always means the server. To run the
benchmark instead, name it explicitly:

```bash
cargo run --bin chunk_bench           # debug
cargo run --release --bin chunk_bench # optimized (use this for real numbers)
```

If you ever see *"could not determine which binary to run"*, you're missing the
`--bin` flag (or `default-run` was removed).

---

## 6. Where data lives & how to reset

Everything the server persists is under your home directory:

```
~/.vibervn/context-engine/
├── settings.json          # config: API keys, repo list, options (chmod 600)
├── rocksdb/               # the per-repo SurrealDB databases
└── embeddings/            # cached embedding vectors (avoids re-calling Voyage)
```

- `settings.json` location is **fixed** and not affected by `--data-dir`.
- To wipe the index and start fresh (keeps your settings):
  ```bash
  rm -rf ~/.vibervn/context-engine/rocksdb
  ```
- To wipe *everything* including keys & repo list:
  ```bash
  rm -rf ~/.vibervn/context-engine
  ```
- For a fully isolated experiment that never touches your real data, run with a
  temp dir:
  ```bash
  cargo run -- --data-dir /tmp/ce-sandbox
  ```

---

## 7. Make it actually work: Voyage API key

Indexing and querying call **Voyage AI** to turn code into embedding vectors,
so you need an API key. Sign up at <https://www.voyageai.com> and create a key
(there is a free tier). The default model is `voyage-4-lite`.

> 🔒 **Never paste the key into a tracked file or commit it.** It belongs in
> `settings.json` (which is written `0600`) or passed at runtime. Don't put it
> in this repo.

LLM reranking (Google Gemini or OpenAI-compatible) is **optional** — skip it to
start; semantic search works without it.

### Option A — via the Web UI (easiest)

1. Start the server (`cargo run`) and open <http://127.0.0.1:6699>.
2. Go to **Settings** → paste your Voyage key under the embedding section.
3. Add a repository: paste the **absolute path** of a code folder you want to
   index (e.g. `/Users/you/code/some-project`).
4. Save. Indexing starts automatically; watch progress in the UI.

### Option B — via the REST API (scriptable)

The config endpoint takes the **whole** settings object, so the safe pattern is
**GET → edit → PUT**. Easiest with [`jq`](https://jqlang.github.io/jq/)
(`brew install jq`):

```bash
# 1. fetch current settings
curl -s http://127.0.0.1:6699/api/config -o /tmp/ce-config.json

# 2. set your Voyage key + add a repo path (edit the values)
jq '.embedding.api_keys = ["YOUR_VOYAGE_KEY"]
    | .repos = ["/Users/you/code/some-project"]' \
   /tmp/ce-config.json > /tmp/ce-config.new.json

# 3. push it back — this persists to settings.json AND triggers indexing of
#    any newly-added repo that exists on disk
curl -s -X PUT http://127.0.0.1:6699/api/config \
     -H 'Content-Type: application/json' \
     --data @/tmp/ce-config.new.json
```

(The required top-level keys in that JSON are `version`, `repos`, `embedding`,
and `llm`; everything else has sensible defaults. Starting from the GET output
guarantees they're all present.)

---

## 8. Index a repo & check status

If you added the repo via the UI or the PUT above, indexing already started.
Check progress:

```bash
curl -s http://127.0.0.1:6699/api/index-status | jq
```

You can also trigger indexing for all configured repos:

```bash
curl -s -X POST http://127.0.0.1:6699/api/index-all
```

Indexing walks the repo, extracts symbols + call edges with tree-sitter, embeds
chunks via Voyage (cached on disk), and stores everything in SurrealDB. The
first index of a large repo takes a while; re-indexes are incremental (only
changed files) and a file watcher re-indexes on save automatically.

---

## 9. Run a query

### Via the Web UI

Use the search / test console in the UI, scope it to your repo, and search in
plain English (e.g. *"where is the function that handles user authentication?"*).

### Via the REST API

`/api/query` requires a `repo` (the absolute path you indexed) and a `query`.
`top_k` (default 30) and `rerank` (default true) are optional:

```bash
curl -s -X POST http://127.0.0.1:6699/api/query \
     -H 'Content-Type: application/json' \
     -d '{
           "query": "where do we open the database connection?",
           "repo": "/Users/you/code/some-project",
           "top_k": 10,
           "rerank": false
         }' | jq
```

You'll get back ranked code snippets with `file#Lstart-end` ranges and
caller/callee context. Set `"rerank": true` only if you've configured an LLM
key; otherwise leave it `false`.

> Tip: you can sharpen queries with field filters baked into the query string:
> `kind:function`, `lang:rust`, `path:src/api`, `name:Handler`.

---

## 10. Day-to-day dev workflow

```bash
cargo fmt                 # auto-format (run before committing)
cargo clippy              # lints — catch common mistakes; fix warnings it flags
cargo build               # quick incremental compile check
cargo test config         # run a focused slice of tests while iterating
```

Auto-rebuild & restart the server on file save (install once with
`cargo install cargo-watch`):

```bash
cargo watch -x run        # works thanks to default-run pinning the server
```

Faster compile feedback without producing a binary:

```bash
cargo check               # type-checks only — quickest "does it compile?" loop
```

---

## 11. Troubleshooting

| Symptom | Cause & fix |
|---------|-------------|
| `linker 'cc' not found` / C errors building tree-sitter | Xcode CLT missing → `xcode-select --install`. |
| `error: edition 2024 is unstable` / feature errors | Rust too old → `rustup update stable`, confirm `rustc --version ≥ 1.85`. |
| `cargo test` shows 7 failures | **Expected on macOS** — the `file_retrieval_db_key_*` Windows path tests (see §3b). Not a problem. |
| `could not determine which binary to run` | You're running the bench path without `--bin`; use `cargo run` for the server or `cargo run --bin chunk_bench`. |
| `Address already in use` when starting | Port 6699 is taken (old instance still running). Stop it, or run `cargo run -- --port 8080`. |
| Query returns `No embedding API keys configured` | Add your Voyage key (§7). |
| Query returns `A repository is required` | Pass `"repo": "<absolute path>"` in the body; it must match an indexed repo. |
| Query returns `No repositories configured` | Add a repo path in Settings / via PUT `/api/config` first. |
| `settings.json ... was written by a newer version` | The on-disk config is from a newer build. Use a matching binary, or back up & delete `~/.vibervn/context-engine/settings.json`. |
| RocksDB "lock" / "IO error: lock hold by current process" | Two instances share one `--data-dir`. Give each its own, or stop the other instance. |
| Indexing seems stuck / want detail | Run with `RUST_LOG=context_engine_rs=debug cargo run` and watch the logs. |

---

## 12. What to read next

- `README.md` — feature list, supported-language table, and the full flow
  diagram.
- `CLAUDE.md` — module map and architecture summary (boot → routes → indexing →
  query → MCP) for when you start changing code.
