# Custom LLM provider ("Custom") — design

**Date:** 2026-07-16
**Status:** Approved (design)
**Area:** LLM Keys settings — rerank/index LLM provider selection

## Problem

The **LLM Keys** settings section offers a Provider dropdown with two options:
`Google` and `OpenAI`. Users who run their own OpenAI-compatible endpoint (a
proxy such as 9router, or a local server like Ollama / LM Studio / vLLM /
OpenRouter serving models such as GLM) can technically already use it — by
selecting **OpenAI** and filling the hidden **"OpenAI Base URL"** field — but
that path is undiscoverable and mislabeled ("OpenAI") for what is really a
user-controlled custom endpoint.

We want a first-class **"Custom"** option in the dropdown that, when selected,
presents an **API endpoint** input and uses the existing **API keys** list for
the key, so pointing the engine at an arbitrary OpenAI-compatible endpoint is
obvious.

### Concrete target case

| Field | Value |
|---|---|
| Provider | Custom |
| API endpoint | `http://proxy-9router-…/v1` |
| Rerank model | e.g. `glm-4.6` |
| API key | `sk-…` (in the API keys list) |

## Approach

**`"custom"` is an alias for the existing OpenAI-compatible code path.** It is a
new *stored provider discriminator* and a new *UI option*, but it reuses the
OpenAI request/response format, the `openai_base_url` config field, and the
shared `api_keys` list. There is **no new protocol, no new config field, and no
settings-version migration**.

Rationale: the backend already speaks OpenAI-compatible to any endpoint via
`LlmConfig::openai_base_url` → `llm::openai::*`. "Custom" only needs to be a
distinct, *remembered* selection that maps onto that path. Keeping `provider`
as the discriminator (value `"custom"`) lets the UI reload into the right state
while the four dispatch sites treat `"custom"` exactly like `"openai"`.

### Rejected alternatives

- **Reuse `"openai"` internally, only rename in the UI.** Rejected: on reload the
  UI cannot tell whether the user picked "OpenAI" or "Custom" — the selection is
  not persisted distinctly.
- **A brand-new non-OpenAI request format / provider arm.** Rejected as YAGNI:
  the real-world endpoints in scope (9router, Ollama, vLLM, OpenRouter, …) are
  all OpenAI-compatible.
- **A dedicated single API-key input for Custom.** Rejected: the existing
  multi-key list already provides masking, rotation across keys, and add/remove.
  A second key store would duplicate that and fragment where keys live.

## Components & changes

### Backend — `src/llm/mod.rs`

- Three provider `match` arms currently keyed on `"openai" =>` become
  `"openai" | "custom" =>`:
  - `call_provider` (single-shot `complete`)
  - `call_provider_with_tools`
  - `call_provider_with_tools_streaming`
  Each already forwards `self.openai_base_url` / `self.openai_force_tool_use`, so
  `"custom"` flows through `openai::*` against the configured endpoint unchanged.
- `provider_supports_structured_output`: add `"custom"` alongside
  `"google" | "openai"` (OpenAI-compatible ⇒ same native-JSON capability; a
  server that lacks it still falls back to the XML path, and the user can also
  untick "Use structured output").
- Update the doc comments on the `LlmClient::openai_base_url` /
  `openai_force_tool_use` fields to read "Honored when `provider == "openai"`
  **or `"custom"`**".

### Backend — `src/config.rs`

- Update the doc comments on `LlmConfig::openai_base_url` and
  `openai_force_tool_use` to note they are also honored for `provider ==
  "custom"`.
- **No new field. No `CURRENT_VERSION` bump. No migration.** `provider` is
  already a free `String` and `openai_base_url` already exists, so existing
  `settings.json` files load unchanged, and a file written with
  `provider: "custom"` deserializes on any build that has these dispatch arms.

### Frontend — `src/assets/index.html`

- Add `<option value="custom">Custom</option>` to `#llm-provider`
  (after the `openai` option, ~line 567).
- Reveal the endpoint wrapper (`#llm-openai-base-url-wrapper`) and the
  force-tool-use wrapper (`#llm-openai-force-tool-use-wrapper`) for `openai`
  **or** `custom`. Update both the initial load logic (~line 2208) and the
  `#llm-provider` `change` handler (~line 3444): the visibility predicate goes
  from `provider !== 'openai'` to `provider !== 'openai' && provider !==
  'custom'`.
- When `custom` is selected, relabel the endpoint field to **"API endpoint"**
  and swap its help text + placeholder; when `openai` is selected, restore the
  "OpenAI Base URL" wording. Implement by toggling the `data-i18n` /
  `data-i18n-ph` keys on the label / input / help `<p>` and re-running the
  existing translate pass on those elements, so language switching keeps working.
- Add i18n keys in en + vi to the flat `{ 'llm.key': { en, vi } }` dictionary:
  - `llm.customEndpoint` — label, e.g. "API endpoint"
  - `llm.customEndpointHelp` — help text, e.g. "OpenAI-compatible endpoint you
    control (proxy, self-hosted server, …). Include the `/v1` path. The API key
    goes in the API keys list below."
  - `llm.customEndpointPlaceholder` — e.g. `https://your-endpoint/v1`
- **API key:** unchanged — the user pastes it into the existing "API keys" list
  (`#llm-keys-list` / `#form-add-llm-key`). This is the second "input" the user
  asked for; it already persists to `cfg.llm.api_keys`.

### Behavior notes

- A blank endpoint under `custom` would fall back to `api.openai.com` (the
  OpenAI default), which contradicts the intent. Show a **soft inline hint**
  ("Enter your endpoint URL") when `custom` is selected and the field is empty.
  Do **not** hard-block saving — the form auto-saves via the existing
  `saveDebounced()` path and a partially-filled form should not throw.
- Plain `http://` endpoints are allowed; the `reqwest` client does not force
  HTTPS. (The 9router example uses `http://`.)
- Structured output, agentic RAG, min-prune-lines, force-tool-use, and key
  rotation all apply to `custom` identically to `openai`.

## Data flow

```
UI (#llm-provider = "custom", #llm-openai-base-url = endpoint,
    #llm-keys-list = [sk-...])
  └─ PUT /api/config  →  Settings.llm { provider:"custom",
                                        openai_base_url:Some(endpoint),
                                        api_keys:[...] }
        └─ LlmClient::new(&settings.llm)
             └─ call_provider* : match "custom" => openai::complete*(
                    model, key, ..., openai_base_url)
                  └─ POST {endpoint}/chat/completions  (OpenAI format)
```

## Testing (`src/config.rs` unit tests, plus `llm/mod.rs`)

1. Round-trip: a `Settings` with `llm.provider = "custom"` and an explicit
   `openai_base_url` survives `write_settings_atomic` → `ensure_dir_and_load`
   with both fields intact and `version == CURRENT_VERSION` (mirrors the
   existing `test_llm_config_round_trips_openai_base_url`).
2. `provider_supports_structured_output("custom")` returns `true`.
3. Deserialize guard: an `llm` block with `"provider":"custom"` and no
   `openai_base_url` parses cleanly with `openai_base_url == None`.

Manual verification (per the `verify` workflow): select **Custom**, enter the
9router endpoint + key + a GLM model, run a query, and confirm the rerank/agent
call hits the custom endpoint (a successful ranked result, or a clear endpoint
error if the server is down — not an `api.openai.com` auth error).

## Out of scope (YAGNI)

- A separate per-provider single-key input box.
- Custom request formats, custom headers, or non-Bearer auth schemes.
- The independent `chat_custom_endpoints` repo-chat model picker — a separate
  feature that already has its own "Custom" endpoints and is untouched here.
- Applying "Custom" to the **embedding** provider (this design covers the
  rerank/index **LLM** provider only).
