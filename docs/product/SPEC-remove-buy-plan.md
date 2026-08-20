# SPEC: Remove the Buy Plan surface

**PRD:** `docs/product/PRD-remove-buy-plan.md` · **Tracker:** issue #1
**Scope decision:** c-safe — remove the UI, the routes, and the proxy module; retain inert config plumbing.

---

## Problem Statement

I run this Context Engine on my own machine, for my own repositories, using my
own API keys. I have never bought a plan and never will.

Every time I open the settings page, it shows me a gold-bordered storefront
advertising packages for sale, a Free Trial button, and an invoice lookup — none
of which mean anything to me. The page header carries a "Buy Plan" button
occupying space I'd rather have for things I actually use.

Worse, opening that page silently contacts a commercial server belonging to the
project I forked from, twice, every single time — once to ask what's for sale,
once to ask whether my machine may still claim a free trial. I never asked for
that. My deployment is private and non-commercial, and it should not be checking
in with somebody else's billing system to render a shop I will never buy from.

I want the whole thing gone: not hidden behind a setting, not switched off by a
flag, but absent — so that the storefront cannot appear, the requests cannot be
made, and the code that makes them is not in my binary.

## Solution

The plan and purchasing surface is removed from this fork in full.

The settings page opens directly onto configuration I actually use, with no
Plans card. The header carries no purchase or quota control. Because the
purchasing UI is gone, nothing triggers a request to the upstream commercial
gateway; because the routes that proxied those requests are also removed,
nothing *can* trigger one, regardless of how the application is started. The
proxy code itself is deleted, so the gateway's address no longer appears in the
compiled binary at all.

Two pieces of inert plumbing stay behind by deliberate choice: the stored (and
always empty) list of purchased plans, and the per-machine identifier computed
at startup. Neither is transmitted anywhere once the routes are gone, and both
are entangled with configuration-file versioning that is not worth disturbing
for a change that is otherwise about removal. This is recorded as a conscious
trade, not an oversight.

Behaviour that is *not* touched: repository indexing, search, chat, provider key
configuration, and every other part of the settings page.

## User Stories

**Owner of a personal instance**

1. As the owner of a personal instance, I want the Plans card absent from my settings page, so that the page shows only configuration I actually use.
2. As the owner of a personal instance, I want no "Buy Plan" button in the header, so that my interface does not advertise a product I am not buying.
3. As the owner of a personal instance, I want no "Search: N left" quota indicator, so that the header does not display a metering concept that does not apply to my own API keys.
4. As the owner of a personal instance, I want no Free Trial section, so that I am never prompted to claim something from a service I do not use.
5. As the owner of a personal instance, I want no invoice lookup field, so that there is no interface implying I have purchases to recover.
6. As the owner of a personal instance, I want no payment method picker, so that no purchase flow can be entered even accidentally.
7. As the owner of a personal instance, I want opening the settings page to make zero requests to the upstream commercial gateway, so that my machine does not announce itself to a third party as a side effect of configuring my own tool.
8. As the owner of a personal instance, I want that to hold no matter which way I start the application, so that I do not have to remember which start-up mode is the "safe" one.
9. As the owner of a personal instance, I want the gateway's address absent from the compiled binary, so that "it cannot call out" is a property of the build rather than a promise about routing.
10. As the owner of a personal instance, I want my network log free of third-party requests while I debug unrelated things, so that what I see is my own traffic.
11. As the owner of a personal instance, I want the settings page to load with no JavaScript errors after the removal, so that deleting the storefront has not quietly broken the rest of the page.
12. As the owner of a personal instance, I want every remaining settings control to keep working, so that the removal costs me no functionality I relied on.
13. As the owner of a personal instance, I want chat to keep working normally, so that a feature I use daily is not collateral damage from removing one I never used.
14. As the owner of a personal instance, I want my existing settings file to keep loading unchanged after upgrading, so that removing a feature does not cost me my configuration.
15. As the owner of a personal instance, I want my configured provider keys untouched, so that indexing and search continue exactly as before.
16. As the owner of a personal instance, I want the interface to stay correct in all three shipped languages, so that removal does not leave a half-translated page.

**Maintainer of the fork**

17. As the maintainer, I want the removal split into two commits — one for the interface, one for the server side — so that if one causes a problem I can reverse it without losing the other.
18. As the maintainer, I want an automated check proving the plan URLs no longer answer, so that a future edit cannot silently reintroduce them.
19. As the maintainer, I want that check to run against both start-up modes, so that a partial removal from only one route table is caught rather than shipped.
20. As the maintainer, I want an automated check proving no plan markup or script survives in the served page, so that I am not relying on having grepped carefully.
21. As the maintainer, I want the existing test suite to stay green, so that I know the removal did not disturb unrelated behaviour.
22. As the maintainer, I want the test that exercised the plan proxy removed alongside the proxy, so that the suite does not test a feature that no longer exists.
23. As the maintainer, I want no unreachable plan code left in the tree, so that a future reader is not misled into thinking the feature is still supported.
24. As the maintainer, I want comments that justified retained code by reference to the plan routes corrected, so that the stated reason for keeping something matches reality.
25. As the maintainer, I want the retained configuration plumbing documented as a deliberate exception, so that its survival reads as a decision rather than a missed spot.
26. As the maintainer, I want the whole change anchored to a written PRD, so that in six months I can recall why the fork diverged here.
27. As the maintainer, I want to know this change will conflict on future upstream merges, so that when it does I recognise it immediately instead of re-investigating.
28. As the maintainer, I want the conflict confined to one interface asset and two route tables, so that resolving it is mechanical rather than exploratory.

**Future reader of the codebase**

29. As someone reading this codebase later, I want no dangling references to a purchasing feature, so that I can trust what I read describes the running system.
30. As someone reading this codebase later, I want the interface asset meaningfully smaller, so that navigating it is less work.
31. As someone reading this codebase later, I want the retained per-machine identifier to carry an accurate explanation of why it still exists, so that I do not delete it on a wrong assumption or preserve it on one.

**Non-goals, stated as stories**

32. As an upstream user who buys plans, I want the upstream project unaffected, so that this fork's decision costs me nothing.
33. As the owner, I do *not* want a setting to re-enable the storefront, so that the removal cannot be undone by accident or by a stray configuration value.

## Implementation Decisions

**Removal, not concealment.** A CSS or feature-flag approach was evaluated and
rejected. It would have produced a far smaller diff and near-zero future merge
friction, but it leaves the code, the translations, and — critically — the
outbound calls in place. The owner chose full removal with the merge cost
stated and accepted.

**Two commits, split by failure mode.** The interface removal and the server-side
removal fail in different ways: the first risks silent script breakage visible
only in a browser, the second risks a failing test. Separating them keeps one
reversible without the other.

**Interface module.** The single-page admin UI asset loses: the header quota and
purchase container; the plans section in its entirety; the plan-related
translation entries across all three shipped languages, including the two header
entries that live outside the main translation block; the boot-time calls that
render purchased plans, start quota polling, fetch packages, and check free-trial
availability; and the full set of plan functions covering packages, checkout,
payment methods, order polling, free trial, purchased-plan storage, usage
display, and header quota rendering.

**Ordering constraint within the interface removal.** Several plan event-listener
registrations resolve their target element and attach a handler in a single
expression, with no guard for a missing element. Removing the markup while
leaving those registrations in place throws during page initialisation and
prevents every subsequently registered listener from attaching — breaking
unrelated parts of the page. Markup and script must therefore be removed
together, in one commit, never in separate steps.

**Cross-feature call site.** The chat turn-completion path calls into the plan
usage refresh so the header quota updates promptly after a turn. With the header
control gone this has nothing to update, and it references functions being
deleted. The call and its explanatory comment are removed; the turn-completion
path otherwise keeps its existing behaviour.

**Route tables.** The plan endpoints are registered in two independent route
tables — one for the standalone server, one for the process-per-project router
mode. Both must be edited. Removing from only one leaves the endpoints live in
the other, which is the specific failure this spec's testing decisions exist to
prevent.

**Proxy module deleted (c-safe).** The router-side plan proxy module is deleted
outright, along with its module declaration, and the corresponding proxy handlers
and their private helpers in the standalone server — including the gateway base
URL constant, the proxy timeout, the HTTP client constructor, and the settings
lookup helper used only by plan handlers. This removes the gateway address from
the compiled binary rather than merely making it unreachable.

**Retained by exception.** The stored purchased-plans setting, its schema
migration, and the per-machine identifier (its salt, its computation, and its
start-up seeding) all remain. The setting participates in a versioned
configuration migration that existing on-disk settings already depend on;
removing it risks the user's live configuration for no user-visible gain. The
identifier is seeded from two start-up paths and asserted by tests unrelated to
plans. Neither is transmitted anywhere once the routes are gone.

**Comment correction.** The router start-up path explains its seeding of the
per-machine identifier by reference to the plan checkout and free-trial routes.
That justification ceases to be true. The comment is corrected in the same
commit that removes the routes, so the tree never contains a comment asserting a
reason that no longer holds.

**No compatibility shim.** Removed endpoints are not replaced with stubs or
redirects. They simply cease to exist and return the router's standard
not-found response.

## Testing Decisions

**What makes a good test here.** Tests assert externally observable behaviour of
the running application — what the server returns over HTTP — never the presence
or absence of particular source constructs. A test that greps the source tree
would pass while the feature still ran; a test that asks the running server is
the real thing. Every assertion below is expressed as a request and its response.

**Seam.** One seam: HTTP against a running application instance. No new test
infrastructure, no browser automation, no unit tests reaching into internals.
Both existing integration suites already start a real instance on an ephemeral
port and drive it with a real HTTP client, so both assertions land on
infrastructure that already exists.

**Assertion 1 — the interface no longer contains the storefront.** Request the
root page and assert the response body contains none of the plan section
identifier, the header purchase button identifier, the plan translation key
prefix, or the plan endpoint path prefix. The admin UI is embedded into the
binary at compile time, so the response body is the authoritative artefact: if a
fragment survived the deletion, it appears here. This single assertion covers the
entire interface removal.

**Assertion 2 — the endpoints no longer answer.** Request a representative plan
endpoint and assert a not-found response. Applied to both start-up modes.

**Both start-up modes are exercised.** Assertion 2 is written once in each of the
two existing integration suites, because the route tables are independent. This
is the same seam in both cases, not a second seam; it is duplicated because the
thing under test is duplicated. Assertion 1 needs only the standalone suite, as
both modes serve the identical embedded asset through the same handler.

**Prior art.** Both suites already contain tests that start an instance and issue
requests against configuration, repository, and index-status endpoints, asserting
on status codes and response bodies. The new tests follow that established shape
directly — same harness, same client, same assertion style.

**Removed test.** The integration test exercising the plan proxy against a mock
gateway is deleted along with the proxy, including its mock server setup. It
verifies behaviour that is being deliberately removed; retaining or adapting it
would be testing a feature that no longer exists.

**Manual verification, explicitly not automated.** Nothing at the HTTP seam can
observe a JavaScript runtime error from a missed call site. Before the change is
considered complete, the application is launched and the settings page loaded in
a browser, confirming zero console errors and that settings and chat both still
function. This is a required gate, and it is deliberately not represented as an
automated test, because claiming automated coverage for it would be false.

## Out of Scope

- **Removing the stored purchased-plans setting or its schema migration.** Deferred by decision; it touches configuration versioning that live settings files depend on.
- **Removing the per-machine identifier**, its salt, its computation, or its start-up seeding. Retained for the same reason, plus test entanglement outside this feature.
- **Any change to the upstream project.** This removal is correct for this fork and wrong for upstream; it is never to be contributed back.
- **Browser or end-to-end test automation.** The manual page load stays manual rather than motivating a new test stack for a one-off removal.
- **A configuration flag, environment variable, or build feature to restore the storefront.** Explicitly rejected — reversibility is provided by version control, not by runtime configuration.
- **Redesign of the settings page** to fill the space the Plans card occupied. Layout reflow is whatever naturally results.
- **Any change to indexing, search, chat, or provider key handling.**

## Further Notes

**Merge conflicts are expected and accepted.** The interface asset is upstream's
most frequently modified file — the majority of recent commits touching it came
from upstream. Roughly nine hundred deleted lines guarantee a conflict in that
file on essentially every future upstream pull, and the removal will need
re-applying each time. This was put to the owner explicitly, priced, and
accepted. It is recorded here so that when the first conflict appears it is
recognised as a known consequence rather than investigated afresh.

**On the corrected traffic baseline.** An earlier estimate held that the page
polled the gateway every thirty seconds. It does not: that poll is conditional on
owning a plan, so on an instance with none the timer fires without sending
anything. The actual traffic is two unconditional requests per settings page
load. The correction lowers the measured baseline; it does not alter the decision,
since the target was always zero.

**No user data was ever at stake.** The two automatic requests carry no keys, no
repository information, and no machine identifier. What they disclose is that an
instance is running, and its IP address. The identifier is transmitted only on an
explicit purchase or trial claim, neither of which has occurred on this instance.

**Verification gate.** Test suite green; settings page loads with zero console
errors; served page contains no plan identifiers; plan endpoints return not-found
in both start-up modes; settings and chat both functional.
