#!/usr/bin/env bash
# Rerank-stage benchmark — the one command behind bench-results/rerank/.
#
# WHY: an A/B of single-shot rerankers is only meaningful with agentic RAG off
# (agentic reranking is a tool-calling loop a decision model cannot serve), and
# flipping that in the owner's real settings.json would leak into every tool that
# uses the engine. So this boots a PRIVATE server whose settings home is a temp
# copy of the real one with agentic_rag=false (and optionally a different rerank
# model), while reusing the real on-disk index and embedding cache through
# --data-dir / --embeddings-dir. Nothing is re-indexed and the real settings are
# never written.
#
# Usage:
#   scripts/rerank_bench.sh <label> [repo_path]
# Env:
#   RERANK_MODEL  override llm.rerank_model for this run only
#   COMPARE       path to a prior artifact; prints cross-run deltas (the A/B axis)
#   TOP_K         default 10
#   PORT          default 7911
#   SETTINGS_HOME default $HOME (reads $SETTINGS_HOME/.vibervn/context-engine/settings.json)
#   NOTE          free text stamped into the artifact (e.g. why a model was substituted)
#
# Output:
#   bench-results/rerank/<label>.json, stamped with a `provenance` block: the
#   reranker the real settings are configured with, what this run overrode, the
#   corpus commit, and the command that reproduces it. Without that block a run
#   taken on a substitute model reads exactly like one taken on the real config.
# Exit code: chunk_bench's. 2 = refused (agentic on), 3 = reranker skipped the
# first query (quota, key, endpoint) and nothing was written.
#
# Caveat: RocksDB allows one process per data dir. Stop any running engine that
# uses the same data dir first, or the private server will fail to open it.
#
# Deps: bash, curl, jq, cargo.
set -euo pipefail

LABEL="${1:?usage: rerank_bench.sh <label> [repo_path]}"
CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="${2:-$CRATE_DIR}"
TOP_K="${TOP_K:-10}"
PORT="${PORT:-7911}"
URL="http://127.0.0.1:${PORT}"
REAL_HOME="${SETTINGS_HOME:-$HOME}"
REAL_CE="${REAL_HOME}/.vibervn/context-engine"
OUT_DIR="${CRATE_DIR}/bench-results/rerank"
OUT_JSON="${OUT_DIR}/${LABEL}.json"
mkdir -p "$OUT_DIR"

[ -f "${REAL_CE}/settings.json" ] || { echo "ERROR: no settings at ${REAL_CE}/settings.json" >&2; exit 1; }

# The temp home holds a copy of settings.json, API keys included: owner-only,
# and always removed on exit.
TMP_HOME="$(mktemp -d)"
chmod 700 "$TMP_HOME"
SERVER_PID=""
cleanup() {
  local code=$?
  if [ -n "$SERVER_PID" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  if [ "$code" -ne 0 ] && [ -f "${TMP_HOME}/server.log" ]; then
    echo "----- server.log (tail) -----" >&2; tail -20 "${TMP_HOME}/server.log" >&2 || true
  fi
  rm -rf "$TMP_HOME"
}
trap cleanup EXIT

mkdir -p "${TMP_HOME}/.vibervn/context-engine"
jq --arg d "$REAL_CE" --arg repo "$REPO" --arg model "${RERANK_MODEL:-}" '
  .llm.agentic_rag = false
  | (if $model != "" then .llm.rerank_model = $model else . end)
  | .data_dir = (.data_dir // $d)
  | .embeddings_dir = (.embeddings_dir // ($d + "/embeddings"))
  | .repos = [$repo]
' "${REAL_CE}/settings.json" > "${TMP_HOME}/.vibervn/context-engine/settings.json"
chmod 600 "${TMP_HOME}/.vibervn/context-engine/settings.json"

echo "[rerank_bench] building release binaries ..."
(cd "$CRATE_DIR" && cargo build --release --bin context-engine-rs --bin chunk_bench)

# If something already listens on PORT, our server fails to bind but the
# readiness probe below would still succeed against the stranger, and the run
# would silently measure the wrong server.
if curl -sS -m 2 -o /dev/null "${URL}/" 2>/dev/null; then
  echo "ERROR: ${URL} is already in use; set PORT to a free port" >&2
  exit 1
fi

echo "[rerank_bench] booting private server on ${URL} (agentic_rag=false${RERANK_MODEL:+, model=$RERANK_MODEL}) ..."
"${CRATE_DIR}/target/release/context-engine-rs" --port "$PORT" --home-dir "$TMP_HOME" \
  > "${TMP_HOME}/server.log" 2>&1 &
SERVER_PID=$!
ready=""
for _ in $(seq 1 30); do
  kill -0 "$SERVER_PID" 2>/dev/null || { echo "ERROR: private server exited during startup" >&2; exit 1; }
  if curl -fsS -m 2 "${URL}/api/index-status" >/dev/null 2>&1; then ready=1; break; fi
  sleep 1
done
[ -n "$ready" ] || { echo "ERROR: server did not come up" >&2; exit 1; }

ARGS=("$REPO" "$URL" "$OUT_JSON" --rerank-ab --label "$LABEL" --top-k "$TOP_K")
[ -n "${COMPARE:-}" ] && ARGS+=(--compare "$COMPARE")
"${CRATE_DIR}/target/release/chunk_bench" "${ARGS[@]}"

REPRODUCE="scripts/rerank_bench.sh ${LABEL} ${REPO}"
[ -n "${RERANK_MODEL:-}" ] && REPRODUCE="RERANK_MODEL=${RERANK_MODEL} ${REPRODUCE}"
jq --slurpfile real "${REAL_CE}/settings.json" \
   --arg model "${RERANK_MODEL:-}" --arg note "${NOTE:-}" --arg cmd "$REPRODUCE" \
   --arg sha "$(git -C "$REPO" rev-parse HEAD 2>/dev/null || echo unknown)" '
  .reproduce_cmd = $cmd
  | .provenance = {
      configured_reranker: {
        provider: $real[0].llm.provider,
        model: $real[0].llm.rerank_model,
        agentic_rag: $real[0].llm.agentic_rag
      },
      overrides: ({agentic_rag: false}
        + (if $model != "" then {rerank_model: $model} else {} end)),
      corpus_commit: $sha,
      note: (if $note != "" then $note else null end)
    }
' "$OUT_JSON" > "${OUT_JSON}.tmp" && mv "${OUT_JSON}.tmp" "$OUT_JSON"
echo "[rerank_bench] wrote ${OUT_JSON}"
