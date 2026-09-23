pub mod google;
pub mod keys;
pub mod openai;

use crate::config::LlmConfig;
use anyhow::{Result, bail};
use keys::{Failure, KeyRing};
use reqwest::Client;
use std::time::Duration;

// ─── Shared types for tool-calling ───────────────────────────────────────

/// A tool definition passed to the LLM.
#[derive(Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A single tool call the model wants to make.
#[derive(Clone, Debug, Default)]
pub struct ToolCall {
    pub name: String,
    pub id: Option<String>,
    pub args: serde_json::Value,
    /// Gemini 2.5/3.x: an opaque `thoughtSignature` attached (at the part level,
    /// as a sibling of `functionCall`) when the model emits a tool call while
    /// thinking is active. It MUST be echoed verbatim when this model turn is
    /// replayed in the conversation history, or the next request 400s with
    /// "Function call is missing a thought_signature". `None` for providers /
    /// models that don't produce one (OpenAI, non-thinking Gemini).
    pub thought_signature: Option<String>,
}

/// A tool result to send back to the model.
#[derive(Clone)]
pub struct ToolResult {
    pub name: String,
    pub id: Option<String>,
    pub content: String,
}

/// Conversation messages for multi-turn tool-calling.
pub enum ChatMessage {
    User(String),
    /// A plain-text assistant turn (no tool calls). Used to replay prior answers
    /// when continuing a multi-turn chat conversation.
    Model(String),
    ModelToolCalls(Vec<ToolCall>),
    ToolResults(Vec<ToolResult>),
}

/// Unified result from a tool-calling turn.
pub enum ToolTurnResult {
    Text(String),
    ToolCalls(Vec<ToolCall>),
}

// ─── LlmClient ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct LlmClient {
    provider: String,
    model: String,
    keys: KeyRing,
    http: Client,
    use_structured_output: bool,
    /// Custom OpenAI-compatible endpoint. Honored when `provider == "openai"`
    /// or `provider == "custom"`; ignored for other providers. `None` /
    /// blank → the OpenAI client falls back to `api.openai.com`.
    /// Normalization (base form vs full URL) happens centrally in
    /// `openai::chat_url`.
    openai_base_url: Option<String>,
    /// Send `tool_choice: "required"` even for custom OpenAI endpoints.
    openai_force_tool_use: bool,
}

/// Whether `provider` has a native JSON output mode the reranker can request.
fn provider_supports_structured_output(provider: &str) -> bool {
    // "custom" is OpenAI-compatible endpoint alias — same native-JSON path.
    matches!(provider, "google" | "openai" | "custom")
}

impl LlmClient {
    /// Create a new client. Returns None if api_keys is empty.
    pub fn new(config: &LlmConfig) -> Option<Self> {
        if config.api_keys.is_empty() {
            return None;
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .ok()?;
        Some(Self {
            provider: config.provider.clone(),
            model: config.rerank_model.clone(),
            keys: KeyRing::new(config.api_keys.clone()),
            http,
            use_structured_output: config.use_structured_output,
            openai_base_url: config.openai_base_url.clone(),
            openai_force_tool_use: config.openai_force_tool_use,
        })
    }

    /// Whether this client will request native JSON output for reranking.
    pub fn structured_output_active(&self) -> bool {
        if !self.use_structured_output {
            return false;
        }
        if provider_supports_structured_output(&self.provider) {
            true
        } else {
            tracing::warn!(
                provider = %self.provider,
                "use_structured_output is enabled but provider has no native JSON mode; \
                 falling back to XML rerank path"
            );
            false
        }
    }

    /// Dispatch to the provider-specific completion function.
    async fn call_provider(
        &self,
        system: &str,
        user: &str,
        temperature: f32,
        structured: bool,
        key: &str,
    ) -> Result<String> {
        match self.provider.as_str() {
            "google" => {
                google::complete(
                    &self.http,
                    &self.model,
                    key,
                    system,
                    user,
                    temperature,
                    structured,
                )
                .await
            }
            "openai" | "custom" => {
                openai::complete(
                    &self.http,
                    &self.model,
                    key,
                    system,
                    user,
                    temperature,
                    structured,
                    self.openai_base_url.as_deref(),
                )
                .await
            }
            other => bail!("unsupported LLM provider: {other}"),
        }
    }

    /// Send a completion request to the configured LLM provider.
    /// Rotates through all keys on failure; keys that hit a rate limit (429)
    /// are excluded from the retry pass so the request isn't wasted on a
    /// known-exhausted quota.
    pub async fn complete(
        &self,
        system: &str,
        user: &str,
        temperature: f32,
        structured: bool,
    ) -> Result<String> {
        self.keys
            .send("LLM call", |key| async move {
                self.call_provider(system, user, temperature, structured, &key)
                    .await
            })
            .await
    }

    /// Dispatch to the provider-specific tool-calling function.
    #[allow(clippy::too_many_arguments)]
    async fn call_provider_with_tools(
        &self,
        system: &str,
        contents: &[ChatMessage],
        tools: &[ToolDef],
        temperature: f32,
        force_tool_use: bool,
        key: &str,
        prompt_cache_key: Option<&str>,
    ) -> Result<ToolTurnResult> {
        match self.provider.as_str() {
            "google" => {
                let r = google::complete_with_tools(
                    &self.http,
                    &self.model,
                    key,
                    system,
                    contents,
                    tools,
                    temperature,
                    force_tool_use,
                )
                .await?;
                match r {
                    google::ToolTurnResult::Text(t) => Ok(ToolTurnResult::Text(t)),
                    google::ToolTurnResult::ToolCalls(calls) => Ok(ToolTurnResult::ToolCalls(
                        calls
                            .into_iter()
                            .map(|c| ToolCall {
                                name: c.call.name,
                                id: c.call.id,
                                args: c.call.args,
                                thought_signature: c.thought_signature,
                            })
                            .collect(),
                    )),
                }
            }
            "openai" | "custom" => {
                let r = openai::complete_with_tools(
                    &self.http,
                    &self.model,
                    key,
                    system,
                    contents,
                    tools,
                    temperature,
                    force_tool_use,
                    prompt_cache_key,
                    self.openai_base_url.as_deref(),
                    self.openai_force_tool_use,
                )
                .await?;
                match r {
                    openai::ToolTurnResult::Text(t) => Ok(ToolTurnResult::Text(t)),
                    openai::ToolTurnResult::ToolCalls(calls) => Ok(ToolTurnResult::ToolCalls(
                        calls
                            .into_iter()
                            .map(|c| {
                                let args = serde_json::from_str(&c.function.arguments)
                                    .unwrap_or(serde_json::Value::Object(Default::default()));
                                ToolCall {
                                    name: c.function.name,
                                    id: Some(c.id),
                                    args,
                                    thought_signature: None,
                                }
                            })
                            .collect(),
                    )),
                }
            }
            other => bail!("unsupported LLM provider for tool-calling: {other}"),
        }
    }

    /// Send a tool-calling request with key rotation + retry.
    ///
    /// `force_tool_use`: when true, the provider is told the model MUST emit a
    /// tool call and may NOT reply with prose (Gemini `mode:ANY`, OpenAI
    /// `tool_choice:required`). The agentic loop sets this while no chunk has
    /// been committed yet, so the model cannot answer the question directly
    /// instead of selecting chunks. Once a chunk is added it flips to false so
    /// the agent can finish with a text summary.
    pub async fn complete_with_tools(
        &self,
        system: &str,
        contents: &[ChatMessage],
        tools: &[ToolDef],
        temperature: f32,
        force_tool_use: bool,
        prompt_cache_key: Option<&str>,
    ) -> Result<ToolTurnResult> {
        self.keys
            .send("LLM tool-call", |key| async move {
                self.call_provider_with_tools(
                    system,
                    contents,
                    tools,
                    temperature,
                    force_tool_use,
                    &key,
                    prompt_cache_key,
                )
                .await
            })
            .await
    }
}

// ─── Streaming tool-calling ───────────────────────────────────────────────

/// Sink for streamed assistant text deltas. Each call carries the next token(s)
/// the model produced. Decoupled from any higher-level event type so the `llm`
/// module stays self-contained; the chat layer maps these into SSE events.
pub type TokenSink<'a> = dyn Fn(&str) + Send + Sync + 'a;

impl LlmClient {
    /// Dispatch to the provider-specific streaming tool-calling function.
    #[allow(clippy::too_many_arguments)]
    async fn call_provider_with_tools_streaming(
        &self,
        system: &str,
        contents: &[ChatMessage],
        tools: &[ToolDef],
        temperature: f32,
        force_tool_use: bool,
        key: &str,
        prompt_cache_key: Option<&str>,
        on_token: &TokenSink<'_>,
        started: &std::sync::atomic::AtomicBool,
    ) -> Result<ToolTurnResult> {
        match self.provider.as_str() {
            "google" => {
                let r = google::complete_with_tools_streaming(
                    &self.http,
                    &self.model,
                    key,
                    system,
                    contents,
                    tools,
                    temperature,
                    force_tool_use,
                    on_token,
                    started,
                )
                .await?;
                match r {
                    google::ToolTurnResult::Text(t) => Ok(ToolTurnResult::Text(t)),
                    google::ToolTurnResult::ToolCalls(calls) => Ok(ToolTurnResult::ToolCalls(
                        calls
                            .into_iter()
                            .map(|c| ToolCall {
                                name: c.call.name,
                                id: c.call.id,
                                args: c.call.args,
                                thought_signature: c.thought_signature,
                            })
                            .collect(),
                    )),
                }
            }
            "openai" | "custom" => {
                let r = openai::complete_with_tools_streaming(
                    &self.http,
                    &self.model,
                    key,
                    system,
                    contents,
                    tools,
                    temperature,
                    force_tool_use,
                    prompt_cache_key,
                    self.openai_base_url.as_deref(),
                    self.openai_force_tool_use,
                    on_token,
                    started,
                )
                .await?;
                match r {
                    openai::ToolTurnResult::Text(t) => Ok(ToolTurnResult::Text(t)),
                    openai::ToolTurnResult::ToolCalls(calls) => Ok(ToolTurnResult::ToolCalls(
                        calls
                            .into_iter()
                            .map(|c| {
                                let args = serde_json::from_str(&c.function.arguments)
                                    .unwrap_or(serde_json::Value::Object(Default::default()));
                                ToolCall {
                                    name: c.function.name,
                                    id: Some(c.id),
                                    args,
                                    thought_signature: None,
                                }
                            })
                            .collect(),
                    )),
                }
            }
            other => bail!("unsupported LLM provider for tool-calling: {other}"),
        }
    }

    /// Streaming variant of [`complete_with_tools`]. Text deltas are delivered
    /// to `on_token` as they arrive; the final assembled result (full text OR
    /// the requested tool calls) is returned.
    ///
    /// Key-rotation rule: a key is only retried if the failure happened
    /// **before any token was streamed** (`started` still false). Once the
    /// provider has begun streaming the body, partial text has already reached
    /// the caller, so retrying on another key would duplicate output — such a
    /// mid-stream failure is returned verbatim for the UI to surface.
    #[allow(clippy::too_many_arguments)]
    pub async fn complete_with_tools_streaming(
        &self,
        system: &str,
        contents: &[ChatMessage],
        tools: &[ToolDef],
        temperature: f32,
        force_tool_use: bool,
        prompt_cache_key: Option<&str>,
        on_token: &TokenSink<'_>,
    ) -> Result<ToolTurnResult> {
        use std::sync::atomic::{AtomicBool, Ordering};
        self.keys
            .send("LLM streaming tool-call", |key| async move {
                let started = AtomicBool::new(false);
                self.call_provider_with_tools_streaming(
                    system,
                    contents,
                    tools,
                    temperature,
                    force_tool_use,
                    &key,
                    prompt_cache_key,
                    on_token,
                    &started,
                )
                .await
                .map_err(|e| {
                    // Tokens already emitted on this key: retrying would
                    // duplicate output, so the error goes to the caller.
                    if started.load(Ordering::Relaxed) {
                        Failure::Stop(e)
                    } else {
                        Failure::Retry(e)
                    }
                })
            })
            .await
    }
}

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
