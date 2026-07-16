# Custom LLM Provider Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a first-class **"Custom"** option to the LLM Keys Provider dropdown that points the rerank/index LLM at any OpenAI-compatible endpoint (e.g. a 9router proxy serving GLM), reusing the existing OpenAI-compatible code path.

**Architecture:** `"custom"` is an alias for the existing `"openai"` provider path. The backend adds `"custom"` to the four provider-dispatch sites so it flows through `llm::openai::*` using the already-present `openai_base_url` field. The frontend adds the dropdown option and, when selected, relabels the existing base-URL field to "API endpoint"; the API key reuses the existing multi-key list. No new config field, no settings-version migration.

**Tech Stack:** Rust (axum server, `reqwest`), single-file vanilla-JS frontend (`src/assets/index.html`, embedded via `include_str!`).

## Global Constraints

- **No new `LlmConfig` field and no `CURRENT_VERSION` bump.** `provider` is already a free `String`; `openai_base_url` already exists. Existing `settings.json` files must load unchanged.
- **Reuse the existing OpenAI-compatible protocol** — do not add a new request/response format, custom headers, or non-Bearer auth.
- **The API key uses the existing "API keys" list** (`#llm-keys-list` / `cfg.llm.api_keys`) — do not add a separate single-key input.
- **i18n has THREE languages: `en`, `vi`, `zh`.** Every new i18n key MUST include all three (correction to the design doc, which said two). Dictionary shape: `'key': { en: \`...\`, vi: \`...\`, zh: \`...\` }`.
- The rerank/index LLM provider only. Do NOT touch the embedding provider or the independent `chat_custom_endpoints` chat-model picker.
- Frontend has no JS test harness; frontend tasks are verified by `cargo build` (the HTML is compiled in via `include_str!`) plus manual browser checks.

---

### Task 1: Backend — alias `"custom"` to the OpenAI-compatible path

**Files:**
- Modify: `src/llm/mod.rs` (provider match arms ~156, ~272, ~442; `provider_supports_structured_output` ~80-82; `LlmClient` field docs ~70-76)
- Modify: `src/config.rs` (doc comments ~327-339; add tests in the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `LlmConfig { provider: String, openai_base_url: Option<String>, openai_force_tool_use: bool, api_keys: Vec<String>, rerank_model: String, .. }` (existing).
- Produces: `LlmClient` dispatch treats `provider == "custom"` identically to `"openai"`; `provider_supports_structured_output("custom") == true`.

- [ ] **Step 1: Write the failing backend tests**

Add these three tests inside the existing `#[cfg(test)] mod tests { ... }` block at the bottom of `src/config.rs` (it already imports `super::*`, `TempDir`, `fs`, `write_settings_atomic`, `ensure_dir_and_load`, `config_path`, `CURRENT_VERSION` — used by the neighbouring `test_llm_config_round_trips_openai_base_url`):

```rust
    /// A `custom` provider round-trips through write + migration-aware reload
    /// with its endpoint intact. `custom` reuses `openai_base_url`, so this is
    /// the same shape as the openai round-trip, only the provider string differs.
    #[test]
    fn test_llm_config_round_trips_custom_provider() {
        let home = TempDir::new().expect("tempdir");
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().expect("has parent")).expect("create dirs");

        let s = Settings {
            llm: LlmConfig {
                provider: "custom".to_owned(),
                rerank_model: "glm-4.6".to_owned(),
                api_keys: vec!["sk-custom".to_owned()],
                openai_base_url: Some("http://proxy.example/v1".to_owned()),
                ..LlmConfig::default()
            },
            ..Settings::default()
        };
        write_settings_atomic(&path, &s).expect("write");

        let loaded = ensure_dir_and_load(home.path()).expect("load");
        assert_eq!(loaded.llm.provider, "custom");
        assert_eq!(
            loaded.llm.openai_base_url.as_deref(),
            Some("http://proxy.example/v1"),
            "custom endpoint must round-trip through write+load"
        );
        assert_eq!(loaded.version, CURRENT_VERSION);
    }

    /// A `custom` llm block with no `openai_base_url` deserializes cleanly with
    /// the field defaulted to `None` (no migration, no version bump required).
    #[test]
    fn test_llm_config_deserializes_custom_without_base_url() {
        let json = r#"{"provider":"custom","rerank_model":"glm-4.6","api_keys":["k"]}"#;
        let cfg: LlmConfig = serde_json::from_str(json).expect("deserialize custom llm block");
        assert_eq!(cfg.provider, "custom");
        assert!(
            cfg.openai_base_url.is_none(),
            "openai_base_url must default to None for custom"
        );
    }
```

Add this test inside the existing `#[cfg(test)] mod tests` block in `src/llm/mod.rs`. If no `mod tests` exists there yet, create one at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_provider_supports_structured_output() {
        assert!(provider_supports_structured_output("custom"));
        assert!(provider_supports_structured_output("openai"));
        assert!(provider_supports_structured_output("google"));
        assert!(!provider_supports_structured_output("anthropic"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib custom_provider_supports_structured_output test_llm_config_round_trips_custom_provider test_llm_config_deserializes_custom_without_base_url`
Expected: `custom_provider_supports_structured_output` FAILS (assertion: `provider_supports_structured_output("custom")` is false). The two config tests PASS already (they don't depend on dispatch) — that's fine; they guard the no-migration contract. If `mod tests` had to be created in `mod.rs`, confirm it compiles.

- [ ] **Step 3: Add `"custom"` to `provider_supports_structured_output`**

In `src/llm/mod.rs` (~line 81), change:

```rust
fn provider_supports_structured_output(provider: &str) -> bool {
    matches!(provider, "google" | "openai")
}
```

to:

```rust
fn provider_supports_structured_output(provider: &str) -> bool {
    // "custom" is an OpenAI-compatible endpoint alias — same native-JSON path.
    matches!(provider, "google" | "openai" | "custom")
}
```

- [ ] **Step 4: Route `"custom"` through the OpenAI dispatch arms**

In `src/llm/mod.rs` there are three identical arms `"openai" => {` (in `call_provider` ~156, `call_provider_with_tools` ~272, and `call_provider_with_tools_streaming` ~442). Replace **all three** occurrences of the exact line:

```rust
            "openai" => {
```

with:

```rust
            "openai" | "custom" => {
```

(Use a replace-all on the exact string `"openai" => {` — all three occurrences are identical and all three should change. The `other => bail!(...)` fallback arms stay as-is.)

- [ ] **Step 5: Update the field/doc comments to mention `custom`**

In `src/llm/mod.rs` (~lines 70-73), update the `openai_base_url` field doc on `LlmClient`:

```rust
    /// Custom OpenAI-compatible endpoint. Honored when `provider == "openai"`
    /// or `provider == "custom"`; ignored for other providers. `None` / blank →
    /// the OpenAI client falls back to `api.openai.com`. Normalization (base
    /// form vs full URL) happens centrally in `openai::chat_url`.
    openai_base_url: Option<String>,
```

In `src/config.rs` (~lines 327-333), update the `LlmConfig::openai_base_url` doc first sentence:

```rust
    /// Custom OpenAI-compatible endpoint (Ollama, LM Studio, OpenRouter, Azure,
    /// vLLM, etc.). Honored when `provider == "openai"` or `provider ==
    /// "custom"`. `None` / blank → the OpenAI client falls back to
    /// `https://api.openai.com/v1/chat/completions`.
```

(Leave the remaining lines of that comment — the `…/v1` normalization note — unchanged.) In `src/config.rs` (~line 334), update the `openai_force_tool_use` doc opening so it reads "even for custom OpenAI-compatible base URLs (`provider == "openai"` or `"custom"`)". Keep the rest of that comment intact.

- [ ] **Step 6: Run the backend tests + build to verify they pass**

Run: `cargo test --lib custom_provider_supports_structured_output test_llm_config_round_trips_custom_provider test_llm_config_deserializes_custom_without_base_url`
Expected: all three PASS.
Run: `cargo build`
Expected: builds clean (no warnings about unreachable match arms).

- [ ] **Step 7: Commit**

```bash
git add src/llm/mod.rs src/config.rs
git commit -m "feat(llm): route 'custom' provider through OpenAI-compatible path"
```

---

### Task 2: Frontend — "Custom" dropdown option + endpoint relabeling

**Files:**
- Modify: `src/assets/index.html` — dropdown option (~567); base-URL wrapper markup (~570-581); i18n dictionary (~1364-1368); `applyI18n` (~1184-1186); config-load block (~2207-2211); provider `change` + base-url `input` handlers (~3441-3457); new `syncLlmProviderUi` helper inserted before line ~3441.

**Interfaces:**
- Consumes: `cfg.llm.provider`, `cfg.llm.openai_base_url`, `cfg.llm.openai_force_tool_use` (existing globals); `t(key)` translator; `saveNow()` / `saveDebounced()`.
- Produces: global `function syncLlmProviderUi(provider)` that toggles the endpoint wrapper + force-tool-use wrapper + empty-endpoint hint, and sets the endpoint field's label / help / placeholder per `openai` vs `custom`.

- [ ] **Step 1: Add the `Custom` dropdown option**

In `src/assets/index.html` (~line 567), change:

```html
            <option value="google">Google</option>
            <option value="openai">OpenAI</option>
          </select>
```

to:

```html
            <option value="google">Google</option>
            <option value="openai">OpenAI</option>
            <option value="custom">Custom</option>
          </select>
```

- [ ] **Step 2: Give the help `<p>` an id and add the empty-endpoint hint**

In `src/assets/index.html` (~lines 578-581), change:

```html
          <p data-i18n="llm.openaiBaseUrlHelp" class="mt-1 text-xs text-gray-400 dark:text-wiki-text-muted">
            Custom OpenAI-compatible endpoint (Ollama, LM Studio, OpenRouter, Azure, vLLM, etc.). Leave blank to use api.openai.com. Accepts either the base form (…/v1) or the full /chat/completions URL.
          </p>
        </div>
```

to (add `id` to the `<p>`, and a hidden amber hint after it):

```html
          <p id="llm-openai-base-url-help" data-i18n="llm.openaiBaseUrlHelp" class="mt-1 text-xs text-gray-400 dark:text-wiki-text-muted">
            Custom OpenAI-compatible endpoint (Ollama, LM Studio, OpenRouter, Azure, vLLM, etc.). Leave blank to use api.openai.com. Accepts either the base form (…/v1) or the full /chat/completions URL.
          </p>
          <p id="llm-custom-endpoint-hint" data-i18n="llm.customEndpointEmptyHint" class="mt-1 text-xs text-amber-600 dark:text-amber-400 hidden">
            Enter your endpoint URL (e.g. https://your-endpoint/v1).
          </p>
        </div>
```

- [ ] **Step 3: Add the four new i18n keys (en/vi/zh)**

In `src/assets/index.html`, immediately after the `'llm.openaiBaseUrlPlaceholder'` line (~line 1368), insert:

```javascript
  'llm.customEndpoint': { en: `API endpoint`, vi: `API endpoint`, zh: `API 端点` },
  'llm.customEndpointHelp': { en: `OpenAI-compatible endpoint you control (a proxy such as 9router, or a self-hosted server). Include the /v1 path. Put the API key in the API keys list below.`,
                         vi: `Endpoint OpenAI-compatible do bạn kiểm soát (proxy như 9router, hoặc server tự host). Bao gồm path /v1. Đặt API key vào danh sách API keys bên dưới.`,
                         zh: `由您控制的 OpenAI 兼容 endpoint（如 9router 代理或自托管服务器）。需包含 /v1 路径。请将 API key 填入下方的 API keys 列表。` },
  'llm.customEndpointPlaceholder': { en: `https://your-endpoint/v1`, vi: `https://your-endpoint/v1`, zh: `https://your-endpoint/v1` },
  'llm.customEndpointEmptyHint': { en: `Enter your endpoint URL (e.g. https://your-endpoint/v1).`,
                         vi: `Nhập URL endpoint của bạn (vd: https://your-endpoint/v1).`,
                         zh: `请输入您的 endpoint URL（例如 https://your-endpoint/v1）。` },
```

- [ ] **Step 4: Add the `syncLlmProviderUi` helper**

In `src/assets/index.html`, immediately before the line `document.getElementById('llm-provider').addEventListener('change', (e) => {` (~line 3441), insert this function (it mirrors `syncEmbeddingProviderUi` at ~3408):

```javascript
// Reveal + relabel the LLM endpoint field for the selected provider. "openai"
// shows an optional "OpenAI Base URL"; "custom" shows a required "API endpoint".
// Called on load, on provider change, on endpoint input, and from applyI18n()
// after a language switch overwrites the static data-i18n defaults.
function syncLlmProviderUi(provider) {
  const showEndpoint = provider === 'openai' || provider === 'custom';
  const isCustom = provider === 'custom';
  document.getElementById('llm-openai-base-url-wrapper').classList.toggle('hidden', !showEndpoint);
  const inputEl = document.getElementById('llm-openai-base-url');
  const labelEl = document.querySelector('label[for="llm-openai-base-url"]');
  const helpEl = document.getElementById('llm-openai-base-url-help');
  if (labelEl) labelEl.textContent = t(isCustom ? 'llm.customEndpoint' : 'llm.openaiBaseUrl');
  if (helpEl) helpEl.textContent = t(isCustom ? 'llm.customEndpointHelp' : 'llm.openaiBaseUrlHelp');
  inputEl.setAttribute('placeholder', t(isCustom ? 'llm.customEndpointPlaceholder' : 'llm.openaiBaseUrlPlaceholder'));
  const hasEndpoint = !!(inputEl.value || '').trim();
  // Force-tool-use only matters once an endpoint is set.
  document.getElementById('llm-openai-force-tool-use-wrapper').classList.toggle('hidden', !(showEndpoint && hasEndpoint));
  // Soft nudge: Custom selected but no endpoint yet (blank would hit api.openai.com).
  const hintEl = document.getElementById('llm-custom-endpoint-hint');
  if (hintEl) hintEl.classList.toggle('hidden', !(isCustom && !hasEndpoint));
}
```

- [ ] **Step 5: Route the provider `change` handler through the helper**

In `src/assets/index.html` (~lines 3443-3446), change the body of the `llm-provider` `change` listener from:

```javascript
    cfg.llm.provider = e.target.value;
    document.getElementById('llm-openai-base-url-wrapper').classList.toggle('hidden', e.target.value !== 'openai');
    const showForce = e.target.value === 'openai' && !!(cfg.llm.openai_base_url || '').trim();
    document.getElementById('llm-openai-force-tool-use-wrapper').classList.toggle('hidden', !showForce);
    saveNow();
```

to:

```javascript
    cfg.llm.provider = e.target.value;
    syncLlmProviderUi(e.target.value);
    saveNow();
```

- [ ] **Step 6: Route the base-URL `input` handler through the helper**

In `src/assets/index.html` (~lines 3450-3457), the `llm-openai-base-url` `input` listener currently sets `cfg.llm.openai_base_url` then recomputes the force-tool-use visibility inline. Replace its body so it reads:

```javascript
document.getElementById('llm-openai-base-url').addEventListener('input', (e) => {
  if (!cfg) return;
  const v = e.target.value.trim();
  cfg.llm.openai_base_url = v === '' ? null : v;
  syncLlmProviderUi(cfg.llm.provider || 'google');
  saveDebounced();
});
```

- [ ] **Step 7: Route the config-load block through the helper**

In `src/assets/index.html` (~lines 2207-2211), change:

```javascript
        document.getElementById('llm-openai-base-url').value = cfg.llm.openai_base_url || '';
        document.getElementById('llm-openai-base-url-wrapper').classList.toggle('hidden', (cfg.llm.provider || 'google') !== 'openai');
        document.getElementById('llm-openai-force-tool-use').checked = cfg.llm.openai_force_tool_use ?? false;
        const showForceToolUse = (cfg.llm.provider || 'google') === 'openai' && !!(cfg.llm.openai_base_url || '').trim();
        document.getElementById('llm-openai-force-tool-use-wrapper').classList.toggle('hidden', !showForceToolUse);
```

to:

```javascript
        document.getElementById('llm-openai-base-url').value = cfg.llm.openai_base_url || '';
        document.getElementById('llm-openai-force-tool-use').checked = cfg.llm.openai_force_tool_use ?? false;
        syncLlmProviderUi(cfg.llm.provider || 'google');
```

- [ ] **Step 8: Re-apply the LLM provider UI after a language switch**

In `src/assets/index.html`, inside `applyI18n()` right after the existing embedding sync block (~lines 1184-1186):

```javascript
  if (typeof cfg !== 'undefined' && cfg && typeof syncEmbeddingProviderUi === 'function') {
    syncEmbeddingProviderUi(cfg.embedding.provider || 'voyage');
  }
```

add the mirrored LLM block:

```javascript
  if (typeof cfg !== 'undefined' && cfg && typeof syncLlmProviderUi === 'function') {
    syncLlmProviderUi(cfg.llm.provider || 'google');
  }
```

- [ ] **Step 9: Build to verify the HTML compiles in and there are no obvious JS syntax errors**

Run: `cargo build`
Expected: builds clean (the HTML is embedded via `include_str!`, so a broken template still compiles — the real check is manual in Step 10).
Run (JS smoke check, optional but recommended): `node --check <(python3 -c "import re,sys;d=open('src/assets/index.html',encoding='utf-8').read();print('\n'.join(re.findall(r'<script[^>]*>(.*?)</script>', d, re.S)))")` — Expected: no syntax error. If `node` is unavailable, skip and rely on Step 10.

- [ ] **Step 10: Manual verification in the browser**

Run: `cargo run` (or the project's normal launch), open the settings page.
1. Provider dropdown now lists **Google / OpenAI / Custom**.
2. Select **Custom** → the endpoint field appears labeled **"API endpoint"**, placeholder `https://your-endpoint/v1`, and an amber hint "Enter your endpoint URL…" shows while it's blank.
3. Type an endpoint → the hint disappears and the **Force tool_choice** checkbox appears.
4. Switch language (en/vi/zh) with Custom selected → the field stays labeled as the Custom "API endpoint" wording, not "OpenAI Base URL".
5. Switch back to **OpenAI** → label returns to "OpenAI Base URL", hint hidden.
6. Reload the page → Custom selection + endpoint persist (confirms it saved to `cfg.llm.provider`).

- [ ] **Step 11: Commit**

```bash
git add src/assets/index.html
git commit -m "feat(ui): add Custom provider option to LLM Keys"
```

---

### Task 3: End-to-end verification against a real custom endpoint

**Files:** none (verification only).

**Interfaces:** Consumes the running app from Tasks 1-2.

- [ ] **Step 1: Configure the Custom provider**

With the app running: Provider = **Custom**; API endpoint = the 9router URL (`http://proxy-9router-…/v1`); Rerank model = the GLM model name (e.g. `glm-4.6`); add the `sk-…` key in the **API keys** list.

- [ ] **Step 2: Exercise the rerank/agent path**

Run a query in the app (or trigger `POST /api/query`) against an indexed repo so the reranker LLM call fires.
Expected: a ranked result returns, OR a clear error naming the **custom endpoint host** (e.g. connection/HTTP error from `proxy-9router…`). A failure that mentions `api.openai.com` means the base URL was not honored — regression, go back to Task 1 Step 4 / Task 2 Step 6-7.

- [ ] **Step 3: Confirm no config regressions**

Run: `cargo test`
Expected: full suite passes (backward-compat/migration tests included).

- [ ] **Step 4: Final commit (if any verification fixups were needed)**

```bash
git add -A
git commit -m "test: verify custom provider end-to-end"
```

(Skip if Steps 1-3 required no changes.)

---

## Notes for the executor

- **No settings migration** is part of this work. If you find yourself editing `CURRENT_VERSION` or adding a `migrate_*` function, stop — the design explicitly avoids it.
- The three `"openai" => {` arms in `src/llm/mod.rs` are byte-identical; a single replace-all is correct and intended.
- Keep the API key in the existing keys list — do not add a dedicated Custom key input.
