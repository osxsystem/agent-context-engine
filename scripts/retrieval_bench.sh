#!/usr/bin/env bash
# Retrieval benchmark — the one command behind bench-results/retrieval/.
#
# WHY: no retrieval change ships without a per-set A/B. This boots a PRIVATE
# server from the binary under test, indexes the corpus into a private data dir,
# and runs chunk_bench over four sets (doc comments, bare identifiers, identifiers
# inside a sentence in three templates, intent paraphrases) once per --arm. An arm
# is a name plus JSON merged into the /api/query body, so a flag-gated change is
# compared on one warm index with nothing else varying. Rerank stays off.
#
# The private server's settings home is a temp copy of the real settings.json
# (embedding keys included) pointed at a private data dir, so nothing here writes
# the live engine's settings or index. The live data dir is refused outright: the
# LaunchAgent server on :6699 holds its RocksDB lock.
#
# Usage:
#   scripts/retrieval_bench.sh <label> [repo_path] [options]
# Options (each also settable by the env var in brackets):
#   --arm NAME=JSON      repeatable; the first two arms get a paired wins/losses
#                        count. With none, a single arm "base" runs unchanged.
#   --sets LIST          doc,ident,mixed,para (default all four)
#   --top-k N            default 10 [TOP_K]
#   --port N             default 7911, never 6699 [PORT]
#   --data-dir DIR       RocksDB base for the private server. Default: a temp dir,
#                        removed on exit. A given dir is kept, so a second run on
#                        it skips the re-index [DATA_DIR]
#   --embeddings-dir DIR shared embedding cache. Default: the real settings' dir,
#                        else ~/.vibervn/context-engine/embeddings [EMBEDDINGS_DIR]
#   --server-bin PATH    context-engine-rs to boot [SERVER_BIN]. Default: build
#                        this checkout in release mode.
#   --bench-bin PATH     chunk_bench to run [BENCH_BIN]. Default: next to the
#                        server binary.
#   --note TEXT          free text stamped into the artifact [NOTE]
# Env:
#   OUT_DIR        artifact dir, default bench-results/retrieval
#   SETTINGS_HOME  default $HOME; reads $SETTINGS_HOME/.vibervn/context-engine/settings.json
#   INDEX_WAIT     seconds to wait for the corpus to index, default 1800
#
# Output:
#   $OUT_DIR/<label>.json with a `provenance` block (binary hashes, code and
#   corpus commits, data dir, reproduce_cmd). The tables print to stdout and end
#   with `queries_failed=N queries_warming=M`. Rerunning the same label rewrites
#   the artifact, so copy it first if you mean to compare.
# Exit code: chunk_bench's. 2 = refused or bad input, 3 = a query failed or came
# back warming.
#
# Deps: bash, curl, jq, cargo (only when --server-bin is not given).
set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { echo "[retrieval_bench] ERROR: $*" >&2; exit 2; }

LABEL=""
REPO=""
ARMS=()
SETS="doc,ident,mixed,para"
TOP_K="${TOP_K:-10}"
PORT="${PORT:-7911}"
DATA_DIR="${DATA_DIR:-}"
EMBEDDINGS_DIR="${EMBEDDINGS_DIR:-}"
SERVER_BIN="${SERVER_BIN:-}"
BENCH_BIN="${BENCH_BIN:-}"
NOTE="${NOTE:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --arm|--sets|--top-k|--port|--data-dir|--embeddings-dir|--server-bin|--bench-bin|--note)
      [ $# -ge 2 ] || die "$1 needs a value"
      case "$1" in
        --arm) ARMS+=("$2") ;;
        --sets) SETS="$2" ;;
        --top-k) TOP_K="$2" ;;
        --port) PORT="$2" ;;
        --data-dir) DATA_DIR="$2" ;;
        --embeddings-dir) EMBEDDINGS_DIR="$2" ;;
        --server-bin) SERVER_BIN="$2" ;;
        --bench-bin) BENCH_BIN="$2" ;;
        --note) NOTE="$2" ;;
      esac
      shift 2 ;;
    -*) die "unknown option $1" ;;
    *)
      if [ -z "$LABEL" ]; then LABEL="$1"
      elif [ -z "$REPO" ]; then REPO="$1"
      else die "unexpected argument $1"
      fi
      shift ;;
  esac
done
[ -n "$LABEL" ] || die "usage: retrieval_bench.sh <label> [repo_path] [options]"
REPO="${REPO:-$CRATE_DIR}"
[ -d "$REPO" ] || die "repo path ${REPO} is not a directory"
case "$LABEL" in *[!A-Za-z0-9._-]*|"") die "label ${LABEL} must be letters, digits, . _ -" ;; esac
case "$PORT" in *[!0-9]*|"") die "port ${PORT} is not a number" ;; esac

command -v curl >/dev/null || die "curl is required"
command -v jq >/dev/null || die "jq is required"

REAL_HOME="${SETTINGS_HOME:-$HOME}"
REAL_CE="${REAL_HOME}/.vibervn/context-engine"
OUT_DIR="${OUT_DIR:-${CRATE_DIR}/bench-results/retrieval}"
OUT_JSON="${OUT_DIR}/${LABEL}.json"
URL="http://127.0.0.1:${PORT}"
[ -f "${REAL_CE}/settings.json" ] || die "no settings at ${REAL_CE}/settings.json"

# Resolve a path that may not exist yet: resolve its deepest existing ancestor
# with symlinks followed, then re-append the rest.
resolve_path() {
  local p="${1/#\~/$HOME}" rest=""
  while [ ! -e "$p" ] && [ "$p" != "/" ] && [ "$p" != "." ]; do
    rest="/$(basename "$p")${rest}"
    p="$(dirname "$p")"
  done
  echo "$(cd "$p" && pwd -P)${rest}"
}

# The live engine owns this directory. Refuse it, anything inside it, and any
# directory that contains it, before a server or an index is created anywhere.
refuse_live_dir() {
  local live got
  live="$(resolve_path "$REAL_CE")"
  got="$(resolve_path "$1")"
  case "${got}/" in
    "${live}/"*) die "--data-dir ${1} resolves to ${got}, which is the live engine dir ${live} or inside it; use a private dir" ;;
  esac
  case "${live}/" in
    "${got}/"*) die "--data-dir ${1} resolves to ${got}, which contains the live engine dir ${live}; use a private dir" ;;
  esac
}
if [ -n "$DATA_DIR" ]; then refuse_live_dir "$DATA_DIR"; fi
[ "$PORT" != "6699" ] || die "port 6699 belongs to the live engine"

TMP_ROOT="$(mktemp -d)"
chmod 700 "$TMP_ROOT"
OWN_DATA_DIR=""
if [ -z "$DATA_DIR" ]; then
  DATA_DIR="${TMP_ROOT}/data"
  OWN_DATA_DIR=1
fi
mkdir -p "$DATA_DIR"
if [ -z "$EMBEDDINGS_DIR" ]; then
  EMBEDDINGS_DIR="$(jq -r '.embeddings_dir // empty' "${REAL_CE}/settings.json")"
  EMBEDDINGS_DIR="${EMBEDDINGS_DIR:-${REAL_CE}/embeddings}"
fi

SERVER_PID=""
cleanup() {
  local code=$?
  if [ -n "$SERVER_PID" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  if [ "$code" -ne 0 ] && [ -f "${TMP_ROOT}/server.log" ]; then
    echo "----- server.log (tail) -----" >&2; tail -20 "${TMP_ROOT}/server.log" >&2 || true
  fi
  rm -rf "$TMP_ROOT"
}
trap cleanup EXIT

if [ -z "$SERVER_BIN" ]; then
  echo "[retrieval_bench] building release binaries ..."
  (cd "$CRATE_DIR" && cargo build --release --locked --bin context-engine-rs --bin chunk_bench)
  TARGET_DIR="${CARGO_TARGET_DIR:-${CRATE_DIR}/target}"
  SERVER_BIN="${TARGET_DIR}/release/context-engine-rs"
fi
BENCH_BIN="${BENCH_BIN:-$(dirname "$SERVER_BIN")/chunk_bench}"
[ -x "$SERVER_BIN" ] || die "server binary ${SERVER_BIN} is not executable"
[ -x "$BENCH_BIN" ] || die "chunk_bench binary ${BENCH_BIN} is not executable"

# If something already listens on PORT, our server fails to bind but the
# readiness probe below would still succeed against the stranger, and the run
# would silently measure the wrong server.
if curl -sS -m 2 -o /dev/null "${URL}/" 2>/dev/null; then
  die "${URL} is already in use; pass --port with a free port"
fi

TMP_HOME="${TMP_ROOT}/home"
mkdir -p "${TMP_HOME}/.vibervn/context-engine"
chmod 700 "$TMP_HOME"
jq --arg d "$DATA_DIR" --arg e "$EMBEDDINGS_DIR" --arg repo "$REPO" '
  .llm.agentic_rag = false
  | .data_dir = $d
  | .embeddings_dir = $e
  | .repos = [$repo]
' "${REAL_CE}/settings.json" > "${TMP_HOME}/.vibervn/context-engine/settings.json"
chmod 600 "${TMP_HOME}/.vibervn/context-engine/settings.json"

echo "[retrieval_bench] booting private server on ${URL} (data ${DATA_DIR}) ..."
"$SERVER_BIN" --port "$PORT" --home-dir "$TMP_HOME" \
  --data-dir "$DATA_DIR" --embeddings-dir "$EMBEDDINGS_DIR" \
  > "${TMP_ROOT}/server.log" 2>&1 &
SERVER_PID=$!
ready=""
for _ in $(seq 1 60); do
  kill -0 "$SERVER_PID" 2>/dev/null || die "private server exited during startup"
  if curl -fsS -m 2 "${URL}/api/index-status" >/dev/null 2>&1; then ready=1; break; fi
  sleep 1
done
[ -n "$ready" ] || die "server did not come up"

echo "[retrieval_bench] indexing ${REPO} ..."
curl -fsS -m 30 -X POST "${URL}/api/index-all" >/dev/null || die "POST /api/index-all failed"
waited=0
while :; do
  status="$(curl -fsS -m 5 "${URL}/api/index-status" 2>/dev/null || echo '[]')"
  state="$(jq -r '.[0].state // "unknown"' <<<"$status" 2>/dev/null || echo unknown)"
  [ "$(jq -r '.[0].error // empty' <<<"$status" 2>/dev/null)" = "" ] || die "indexing ${REPO} failed: $(jq -r '.[0].error' <<<"$status")"
  if [ "$state" = "idle" ] || [ "$state" = "indexed" ]; then
    [ "$(jq -r '.[0].last_indexed_at // empty' <<<"$status")" = "" ] || break
  fi
  kill -0 "$SERVER_PID" 2>/dev/null || die "private server exited while indexing"
  [ "$waited" -lt "${INDEX_WAIT:-1800}" ] || die "indexing ${REPO} still ${state} after ${waited}s"
  if [ $((waited % 30)) -eq 0 ]; then echo "[retrieval_bench] index state: ${state} (${waited}s)"; fi
  sleep 3
  waited=$((waited + 3))
done

ARGS=("$REPO" "$URL" "$OUT_JSON" --label "$LABEL" --sets "$SETS" --top-k "$TOP_K")
for arm in ${ARMS[@]+"${ARMS[@]}"}; do ARGS+=(--arm "$arm"); done
mkdir -p "$OUT_DIR"
set +e
"$BENCH_BIN" "${ARGS[@]}"
BENCH_CODE=$?
set -e

REPRODUCE="scripts/retrieval_bench.sh $(printf '%q ' "$LABEL" "$REPO")--sets $(printf '%q' "$SETS") --top-k ${TOP_K}"
for arm in ${ARMS[@]+"${ARMS[@]}"}; do REPRODUCE+=" --arm $(printf '%q' "$arm")"; done
[ -n "$OWN_DATA_DIR" ] || REPRODUCE+=" --data-dir $(printf '%q' "$DATA_DIR")"
[ "$OUT_DIR" = "${CRATE_DIR}/bench-results/retrieval" ] || REPRODUCE="OUT_DIR=$(printf '%q' "$OUT_DIR") ${REPRODUCE}"
REPRODUCE+=" --server-bin $(printf '%q' "$SERVER_BIN") --bench-bin $(printf '%q' "$BENCH_BIN")"

sha() { if command -v shasum >/dev/null; then shasum -a 256 "$1"; else sha256sum "$1"; fi | cut -d' ' -f1; }
if [ -f "$OUT_JSON" ]; then
  jq --arg cmd "$REPRODUCE" --arg note "$NOTE" \
     --arg code_commit "$(git -C "$CRATE_DIR" rev-parse HEAD 2>/dev/null || echo unknown)" \
     --arg corpus_commit "$(git -C "$REPO" rev-parse HEAD 2>/dev/null || echo unknown)" \
     --arg server_bin "$SERVER_BIN" --arg server_sha "$(sha "$SERVER_BIN")" \
     --arg bench_bin "$BENCH_BIN" --arg bench_sha "$(sha "$BENCH_BIN")" \
     --arg data_dir "$DATA_DIR" --argjson own_data "$([ -n "$OWN_DATA_DIR" ] && echo true || echo false)" \
     --arg embeddings_dir "$EMBEDDINGS_DIR" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '
    .reproduce_cmd = $cmd
    | .provenance = {
        date: $date,
        code_commit: $code_commit,
        corpus_commit: $corpus_commit,
        server_bin: {path: $server_bin, sha256: $server_sha},
        bench_bin: {path: $bench_bin, sha256: $bench_sha},
        data_dir: {path: $data_dir, private_temp: $own_data},
        embeddings_dir: $embeddings_dir,
        rerank: false,
        note: (if $note != "" then $note else null end)
      }
  ' "$OUT_JSON" > "${OUT_JSON}.tmp" && mv "${OUT_JSON}.tmp" "$OUT_JSON"
  echo "[retrieval_bench] wrote ${OUT_JSON}"
fi
exit "$BENCH_CODE"
