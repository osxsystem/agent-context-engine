# UI Surface

Browser-driven evidence for the matrix rows whose surface is the rendered UI.

Drive with `agent-browser` for live interaction, or with the project's own Playwright/Cypress/browser-mode setup for repeatable runs. The project's harness wins wherever it exists.

## Discover, then design

1. Open the target and walk it: pages, routes, forms, navigation, and the states behind interaction, including modal, empty, loading, and error.
2. Turn the walk into matrix rows like any other behaviour, with viewport and role as extra dimensions.
3. Where the row count justifies it, fan rows out across parallel `general-purpose` agents by area: pages, forms, flows, accessibility, responsive, performance.

## Authentication

Prefer the project's auth helper or fixture. Where none exists, have the user log in manually once, then persist that session state and reuse it for the run.

## Read per row

- **Console**: errors and warnings raised during the interaction.
- **Network**: failed requests, 4xx/5xx, requests fired twice.
- **Layout**: screenshot against the expectation, at each viewport in the row.
- **Interaction**: the flow completes: validation messages, disabled states, focus after action.
- **Accessibility**: keyboard reachability, focus order, labels, contrast.

Screenshot every row and read the image back with the `Read` tool, because the render is the evidence. Save screenshots beside the report and cite their paths in the row.

## Verdict

Each row resolves PASS, FAIL, or UNVERIFIED like any other, and failures go through `triage.md`. Hand the diagnosis back; the caller decides on fixes.
