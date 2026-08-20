# PRD: Remove the Buy Plan surface

**Status:** Approved, not yet implemented
**Repository:** `osxsystem/agent-context-engine` (fork of `nullmastermind/vibervn-context-engine`)
**Version at time of writing:** 0.1.72
**Author:** Do Viet Hung

---

## 1. Summary

The upstream Context Engine ships a monetization surface — a "Plans" card in
Settings, a "Buy Plan" indicator in the header, and seven `/api/plan/*` proxy
routes that forward to upstream's admin gateway at `context-engine.viber.vn`.
This fork is a personal, non-commercial deployment that sells nothing and uses
its owner's own API keys. This document specifies removing that surface from the
fork, in full, and stopping all outbound traffic to upstream's gateway.

---

## 2. Contacts

| Name | Role | Comment |
| ---- | ---- | ------- |
| Do Viet Hung | Owner / Maintainer | Sole decision-maker. Personal fork, personal business. |
| `nullmastermind/vibervn-context-engine` (upstream) | Source project | **Deliberately not consulted.** This divergence is intentional and accepted; the change is never to be pushed upstream. |

---

## 3. Background

**What this is about.** The fork tracks upstream and pulls from it regularly —
most recently merge `af8eb95`, bringing upstream through v0.1.72. Riding along
with that history is upstream's paid-plan feature: package listings, checkout via
bank card/QR and USDT, free-trial claiming, purchased-plan tracking, and a
per-machine dedup identifier. All of it exists to sell access to a hosted search
and reranking service.

None of that applies here. This deployment is personal, sells nothing, and is
configured entirely with the owner's own provider keys. The Plans surface is
therefore dead weight with two concrete costs:

1. **Unwanted outbound traffic.** Every settings page load makes two
   unconditional requests to `context-engine.viber.vn`: one asking which
   packages are for sale, one asking whether this machine may still claim a free
   trial. Both exist only to draw a storefront this deployment will never buy
   from. A third call polls plan usage on a 30-second timer, but it is gated on
   owning a plan — on this instance the timer fires and sends nothing. Neither
   automatic request carries identifying data (no keys, no machine ID, no repo
   information), but both disclose to a third party that the instance is running
   and expose its IP. A personal instance contacting someone else's monetization
   backend is undesirable on principle.
2. **Interface noise.** A gold-bordered "Plans" card and a "Buy Plan" header
   button advertise a purchase flow that will never be used.

**Why now.** The fork has accumulated 12 local commits and has just absorbed a
fresh upstream merge, so the divergence is already established and deliberate.
Doing the removal now — rather than after further upstream merges — means
resolving the conflict surface once, from a known-good state, instead of
inheriting more churn in the same file first.

---

## 4. Objective

Remove upstream's monetization surface from this fork so the settings page shows
only what the owner actually uses, and so the running instance makes no network
calls to upstream's admin gateway.

This benefits the owner in three ways: a settings page free of irrelevant
commercial UI; no third-party heartbeat from a personal machine; and a smaller,
more navigable `index.html`. It aligns with what this fork is for — a private,
self-hosted context engine running on the owner's own keys, not a client for
someone else's paid service.

### Key Results

Measured on the fork's `master` after implementation, before the next
`git pull upstream`:

| # | Result | Target | Baseline |
| - | ------ | ------ | -------- |
| KR1 | Outbound HTTP requests to `context-engine.viber.vn` per settings page load | **0** | **2** — `/api/packages` and `/api/free-trial`, both unconditional at boot. (The 30s usage poll is gated on owning a plan and sends nothing on this instance.) |
| KR2 | JavaScript console errors on settings page load | **0** | 0 (must not regress) |
| KR3 | `plan` / `Plan` identifiers present in the served settings DOM | **0** | 29 functions + 1 section + 1 header block |
| KR4 | `cargo test` result | **green** | green (must not regress) |
| KR5 | `src/assets/index.html` size | **~5,480 lines** (−900) | 6,377 lines |

**Deadline:** this working session, and strictly before the next
`git pull upstream`.

---

## 5. Market Segment(s)

A single segment of one: **the owner of a self-hosted personal Context Engine
instance who supplies their own provider keys.**

The defining job is running local semantic code search over personal
repositories without buying hosted capacity. The constraint that shapes this
work is that the fork must remain *mergeable with upstream* — upstream keeps
shipping features worth pulling, so the removal cannot be so invasive that
future merges become impractical.

Explicitly **out of segment:** users who buy plans from
`context-engine.viber.vn`. They are served by upstream, which retains the
feature untouched.

---

## 6. Value Proposition(s)

**Jobs addressed**

- Configure and operate a private context engine without navigating around a
  storefront for a service you don't use.
- Keep a personal machine from contacting a third-party commercial backend on a
  timer.

**Gains**

- Settings page contains only actionable configuration.
- No outbound calls to upstream's gateway; the routes stop existing rather than
  merely going unused.
- ~900 fewer lines in the largest frontend file, making the rest easier to read
  and edit.

**Pains avoided**

- A "Buy Plan" button that leads to a purchase flow with no purpose here.
- Two pointless third-party requests in the network log on every settings page
  load, cluttering the view while debugging unrelated things.

**Why better than the alternatives.** Two alternatives were considered and
rejected in review:

- *Hide with CSS / a feature flag* — a ~5-line diff that would almost never
  conflict with upstream. Rejected: it leaves ~900 lines of dead code and 40
  orphan translation strings in the tree, and does not stop the polling on its
  own. The owner explicitly chose full removal over merge convenience.
- *Remove only the visible markup* — rejected as actively unsafe. Four listener
  registrations (`index.html:3429`, `:3447`, `:3462`, `:3465`) call
  `getElementById(...).addEventListener(...)` with no null guard; deleting the
  markup without the script throws at load and silently kills every listener
  registered after that point.

---

## 7. Solution

### 7.1 UX

**Before.** The header shows either a "Buy Plan" button or a "Search: N left"
indicator. Clicking either switches to the Settings tab, force-expands the Plans
card, and scrolls to it. Settings contains a gold-bordered collapsible "Plans"
card holding Free Trial, package listings, a payment-method picker, purchased
plans, and invoice lookup.

**After.** The header carries no quota or purchase control. Settings opens
directly onto the owner's own configuration with no Plans card. No flow is
replaced or relocated — the surface is gone, not moved. Chat behaviour is
unchanged apart from no longer refreshing a quota display that no longer exists.

### 7.2 Key Features

The deliverable is a removal, shipped as two commits.

**Commit 1 — remove the Buy Plan UI** (`src/assets/index.html`)

| Region | Content |
| ------ | ------- |
| `:264–265` | Header quota / Buy-Plan container and comment |
| `:444–555` | `<section id="section-buy-plan">` — the Plans card |
| `:1366–1367` | `header.searchLeft`, `header.buyPlan` translations |
| `:1403–1445` | 40 `plan.*` translation keys (en / vi / zh) |
| `:2590–2591` | `renderPurchasedPlans()` and `startQuotaPolling()` boot calls |
| `:2770–3520` | 29 plan functions, including the four unguarded listener registrations |
| `:6172–6174` | Chat `'done'` handler: the `if (hasLivePlan()) fetchAllPlanUsage();` call and its comment |

**Commit 2 — remove the plan proxy routes**

| File | Change |
| ---- | ------ |
| `src/router/routes.rs:10–16` | Drop 7 `/api/plan/*` route registrations |
| `src/server.rs:249–260` | Drop the same 7 at the second registration site |
| `tests/integration.rs:1050–1160` | Delete `test_plan_proxy_forwards_machine_id_and_injects_base_url` and its mock gateway |

**Deliberately retained.** `src/router/plan.rs` and the `plan_*` handlers in
`server.rs` (unreachable once unregistered); the `purchased_plans` settings
field and its v8→v9 migration; `machine_id`, `MACHINE_ID_SALT`, and
`ensure_machine_id`. Removing these would mean editing config schema migrations
that existing on-disk settings already depend on, and would break tests in
`tests/router_integration.rs` and `tests/integration.rs` that have nothing to do
with plans. The cost is real; the benefit is tidiness in files that are never
read.

### 7.3 Technology

Two details shape the work:

- **The routes are registered twice** — in `src/router/routes.rs` (used by
  process-per-project router mode) and again in `src/server.rs`. Both sites must
  be edited or the endpoints stay reachable in one of the two run modes.
- **There is no automated test covering `index.html`.** `cargo test` cannot see
  frontend breakage, which is why verification requires an actual page load.

### 7.4 Assumptions

| # | Assumption | Basis | If wrong |
| - | ---------- | ----- | -------- |
| A1 | The owner has never claimed a free trial or purchased a package on this machine, so no proxy key in the live provider config originates from `viber.vn`. | Stated directly by the owner. | A working provider key could be stranded with no UI to view or re-apply it. Recoverable from `purchased_plans` in settings, which is retained. |
| A2 | Recurring merge conflicts in `index.html` are an accepted cost. | Owner said they do not care about upstream and chose full removal after the cost was put to them explicitly. | Every future `git pull upstream` conflicts in this file and the removal must be re-applied by hand. |
| A3 | Nothing outside the enumerated regions depends on the plan functions. | Full-file sweep for `plan` / `Plan` / `quota` identifiers; the only reference outside the two main blocks was the chat handler at `:6174`, which is included. | A runtime `ReferenceError` on a path not exercised during verification. |
| A4 | The fork will keep pulling from upstream indefinitely. | Owner merged upstream at the start of this session. | If the fork stops tracking upstream, A2's cost disappears entirely and this becomes unambiguously the right call. |
| A5 | Leaving `plan.rs` and the `plan_*` handlers as unreachable dead code is acceptable. | Judged a better trade than editing config migrations. | Dead code lingers; a future reader may be confused about whether plans are supported. |

---

## 8. Release

**First version — this session.** Both commits, landed on the fork's `master`
and pushed to `origin`. Scope is exactly section 7.2. This is the whole product;
there is no phased rollout, no flag, and no staged audience.

Verification gate before the work is called done:

1. `cargo test` passes.
2. The engine launches and the settings page loads with **zero** console errors.
3. The served DOM contains no `plan` identifiers.
4. The Settings tab and chat both still function.

**Deliberately deferred, with no committed timeframe.** Removing
`src/router/plan.rs`, the `plan_*` handlers, the `purchased_plans` settings
field and its v8→v9 migration, and `machine_id`. This becomes worth revisiting
only if the fork stops tracking upstream, at which point the config-migration
risk can be absorbed on a schedule rather than mid-session.

**Not planned.** Any contribution of this change upstream. The removal is
correct for this fork and wrong for the source project.

---

## Provenance

This PRD was produced from a four-round design review covering: the reason for
removal, scope across UI and backend, removal depth versus merge cost,
network-traffic intent, retained state, commit structure, and the verification
bar. Two concerns — recurring merge conflicts (A2) and the absence of a frontend
test net — were raised, overridden by the owner, and are recorded here rather
than re-argued.

**Correction log.** KR1's baseline was initially recorded as "~20 requests per
10-minute session," on the belief that the 30-second usage poll called out
unconditionally. It does not — `pollQuotaUsage` is gated on `hasLivePlan()`, so
on an instance with no purchased plans the timer fires without sending anything.
The real baseline is the 2 unconditional boot-time fetches. Target and decision
are unchanged; only the measured starting point was wrong.

**Next step:** `/to-spec` to turn this into buildable work. Specs deriving from
this document should reference `docs/product/PRD-remove-buy-plan.md`.
