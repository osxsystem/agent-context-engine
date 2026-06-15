#!/usr/bin/env python3
"""Embedding-provider admin for the running context-engine server.

A small CLI wrapping the live HTTP API (default http://127.0.0.1:6699) to
switch the embedding provider, roll back, probe the provider endpoint, and
drive per-repo index operations — without restarting the server (the
EmbedClient is rebuilt from live settings on each run, so PUT /api/config
applies immediately).

Safety properties:
  * `switch`/`rollback` back up settings.json to `settings.json.bak-<provider>-<ts>`
    BEFORE writing, so any change is reversible.
  * API keys are NEVER printed.
  * `switch gemini` moves the Google key from `llm.api_keys` into
    `embedding.api_keys`; `switch voyage` (or `rollback`) restores the
    most-recent Voyage backup's embedding block.

NOTE: switching the embedding model changes the vector dimension, so a repo is
only searchable again after a forced rebuild (`rebuild <repo>`); an incremental
run will NOT re-embed because change detection ignores the model. See
CLAUDE.md / LOCAL_DEV.md.

Examples:
    python3 scripts/embedding_admin.py status
    python3 scripts/embedding_admin.py probe
    python3 scripts/embedding_admin.py switch gemini --model gemini-embedding-001
    python3 scripts/embedding_admin.py rollback
    python3 scripts/embedding_admin.py cancel topology
    python3 scripts/embedding_admin.py rebuild topology
"""
import argparse
import base64
import glob
import json
import os
import sys
import time
import urllib.error
import urllib.request

BASE = os.environ.get("CONTEXT_ENGINE_URL", "http://127.0.0.1:6699")
SETTINGS = os.path.expanduser("~/.vibervn/context-engine/settings.json")


def http(method, path, payload=None, timeout=60):
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(
        BASE + path, data=data,
        headers={"Content-Type": "application/json"}, method=method,
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        raw = r.read().decode()
        return r.status, (json.loads(raw) if raw.strip() else None)


def repo_id(repo: str) -> str:
    """URL-safe base64 (no pad) of the repo path — matches decode_repo_id."""
    return base64.urlsafe_b64encode(repo.encode()).decode().rstrip("=")


def resolve_repo(needle: str, repos):
    """Match a repo by exact path or unique substring."""
    if needle in repos:
        return needle
    hits = [r for r in repos if needle.lower() in r.lower()]
    if len(hits) == 1:
        return hits[0]
    if not hits:
        sys.exit(f"no repo matches {needle!r}; configured: {repos}")
    sys.exit(f"{needle!r} is ambiguous: {hits}")


def backup_settings(tag: str) -> str:
    ts = time.strftime("%Y%m%d-%H%M%S")
    dst = f"{SETTINGS}.bak-{tag}-{ts}"
    with open(SETTINGS) as f:
        data = f.read()
    with open(dst, "w") as f:
        f.write(data)
    os.chmod(dst, 0o600)
    return dst


def latest_backup(provider: str):
    baks = sorted(glob.glob(f"{SETTINGS}.bak-{provider}-*"))
    return baks[-1] if baks else None


# ─── commands ──────────────────────────────────────────────────────────────

def cmd_status(_args):
    _, cfg = http("GET", "/api/config")
    e = cfg["embedding"]
    print(f"embedding: provider={e['provider']} model={e['model']} "
          f"keys={len(e['api_keys'])} concurrency={e.get('embed_concurrency')}")
    for repo in cfg.get("repos", []):
        try:
            _, s = http("GET", f"/api/repos/{repo_id(repo)}/status")
            print(f"  {s.get('state'):<10} {s.get('indexed_files')}/{s.get('total_files'):<6} "
                  f"err={s.get('error')}  {repo}")
        except urllib.error.HTTPError as ex:
            print(f"  status-error HTTP {ex.code}  {repo}")


def cmd_probe(_args):
    """Embed one short text against the CURRENT provider to confirm the key works."""
    cfg = http("GET", "/api/config")[1]
    e = cfg["embedding"]
    key = e["api_keys"][0]
    model = e["model"]
    if e["provider"] == "voyage":
        base = (e.get("voyage_base_url") or "https://api.voyageai.com/v1").rstrip("/")
        url = base + ("" if base.endswith("/embeddings") else "/embeddings")
        body = {"model": model, "input": ["probe"], "input_type": "document"}
        req = urllib.request.Request(url, data=json.dumps(body).encode(),
            headers={"Content-Type": "application/json",
                     "Authorization": f"Bearer {key}"}, method="POST")
    else:  # gemini / google
        url = (f"https://generativelanguage.googleapis.com/v1beta/models/"
               f"{model}:batchEmbedContents?key={key}")
        body = {"requests": [{"model": f"models/{model}",
                              "content": {"parts": [{"text": "probe"}]},
                              "taskType": "RETRIEVAL_DOCUMENT"}]}
        req = urllib.request.Request(url, data=json.dumps(body).encode(),
            headers={"Content-Type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            resp = json.load(r)
        first = (resp.get("data") or resp.get("embeddings"))[0]
        vec = first.get("embedding") or first.get("values") or []
        print(f"PROBE OK — 200, provider={e['provider']} model={model} dim={len(vec)}")
    except urllib.error.HTTPError as ex:
        msg = ex.read().decode().replace(key, "<KEY>")
        print(f"PROBE FAILED — HTTP {ex.code}: {msg[:400]}")


def cmd_switch(args):
    _, cfg = http("GET", "/api/config")
    bak = backup_settings(cfg["embedding"]["provider"])
    print(f"backed up -> {os.path.basename(bak)}")
    prov = args.provider.lower()
    if prov in ("gemini", "google"):
        gkey = cfg["llm"]["api_keys"][0]
        cfg["embedding"]["provider"] = "gemini"
        cfg["embedding"]["model"] = args.model or "gemini-embedding-001"
        cfg["embedding"]["api_keys"] = [gkey]
    elif prov == "voyage":
        src = latest_backup("voyage")
        if not src:
            sys.exit("no voyage backup found to restore keys from; use rollback or set keys manually")
        cfg["embedding"] = json.load(open(src))["embedding"]
        if args.model:
            cfg["embedding"]["model"] = args.model
    else:
        sys.exit(f"unknown provider {prov!r}")
    http("PUT", "/api/config", cfg)
    after = http("GET", "/api/config")[1]["embedding"]
    print(f"switched -> provider={after['provider']} model={after['model']} keys={len(after['api_keys'])}")
    print("NOTE: vector dim changed — run `rebuild <repo>` to make a repo searchable again.")


def cmd_rollback(args):
    src = latest_backup(args.provider)
    if not src:
        sys.exit(f"no backup matching .bak-{args.provider}-*")
    _, cfg = http("GET", "/api/config")
    backup_settings("prerollback")
    cfg["embedding"] = json.load(open(src))["embedding"]
    http("PUT", "/api/config", cfg)
    after = http("GET", "/api/config")[1]["embedding"]
    print(f"rolled back from {os.path.basename(src)} -> "
          f"provider={after['provider']} model={after['model']} keys={len(after['api_keys'])}")


def cmd_cancel(args):
    repos = http("GET", "/api/config")[1]["repos"]
    repo = resolve_repo(args.repo, repos)
    st, resp = http("POST", f"/api/repos/{repo_id(repo)}/cancel-index")
    print(f"cancel {repo} -> {st} {resp}")


def cmd_rebuild(args):
    repos = http("GET", "/api/config")[1]["repos"]
    repo = resolve_repo(args.repo, repos)
    st, resp = http("POST", f"/api/repos/{repo_id(repo)}/rebuild")
    print(f"rebuild {repo} -> {st} {resp}")
    if args.watch:
        rid = repo_id(repo)
        last = None
        while True:
            _, s = http("GET", f"/api/repos/{rid}/status")
            line = f"{s['state']} {s['indexed_files']}/{s['total_files']} err={s['error']}"
            if line != last:
                print(f"[{time.strftime('%H:%M:%S')}] {line}", flush=True)
                last = line
            if s["state"] not in ("indexing", "queued", "pending"):
                break
            time.sleep(5)


def main():
    p = argparse.ArgumentParser(description="Embedding-provider admin for context-engine.")
    sub = p.add_subparsers(dest="cmd", required=True)

    sub.add_parser("status", help="show embedding config + per-repo index status").set_defaults(fn=cmd_status)
    sub.add_parser("probe", help="embed one text against the current provider to test the key").set_defaults(fn=cmd_probe)

    sw = sub.add_parser("switch", help="switch embedding provider (backs up first)")
    sw.add_argument("provider", choices=["gemini", "google", "voyage"])
    sw.add_argument("--model", help="embedding model (default: gemini-embedding-001 for gemini)")
    sw.set_defaults(fn=cmd_switch)

    rb = sub.add_parser("rollback", help="restore the most-recent backup for a provider")
    rb.add_argument("--provider", default="voyage")
    rb.set_defaults(fn=cmd_rollback)

    cn = sub.add_parser("cancel", help="cancel indexing for a repo (path or substring)")
    cn.add_argument("repo")
    cn.set_defaults(fn=cmd_cancel)

    re = sub.add_parser("rebuild", help="force a full rebuild for a repo (path or substring)")
    re.add_argument("repo")
    re.add_argument("--watch", action="store_true", help="poll status until the rebuild finishes")
    re.set_defaults(fn=cmd_rebuild)

    args = p.parse_args()
    try:
        args.fn(args)
    except urllib.error.URLError as e:
        sys.exit(f"cannot reach server at {BASE}: {e}")


if __name__ == "__main__":
    main()
