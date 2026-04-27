//! Agentix-backed streaming path for [`crate::client::ModelClient::stream`].
//!
//! When `wire_api = "agentix"` is set on a model provider, we route the turn
//! through the [`agentix`] crate instead of the OpenAI Responses API. This
//! module owns the two-way translation:
//!
//! - **Inbound** (Codex → agentix): the [`Prompt`] (input items, tools,
//!   base instructions) is rebuilt as an [`agentix::Request`].
//! - **Outbound** (agentix → Codex): the [`agentix::msg::LlmEvent`] stream
//!   is mapped onto [`ResponseEvent`]s and pushed through the standard mpsc
//!   channel that `ModelClient::stream` expects.
//!
//! The current commit is a skeleton: the public entry point is wired up but
//! returns a `not implemented` stream error. Subsequent commits implement
//! request/event translation, tool-call assembly, and provider_data
//! round-tripping for `LlmEvent::AssistantState`.
//!
//! Cf. familiar's `backend/src/worker.rs` for the canonical hand-written
//! LlmEvent → consumer loop this module mirrors.

use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;

use crate::client_common::Prompt;
use crate::client_common::ResponseStream;

/// Stream one model turn through agentix.
///
/// `agentix_provider_id` is the value from
/// `ModelProviderInfo::agentix_provider` (e.g. `"anthropic"`, `"openai"`,
/// `"deepseek"`, `"gemini"`, `"kimi"`, `"glm"`, `"minimax"`, `"grok"`,
/// `"openrouter"`, `"claude-code"`).
pub async fn stream_via_agentix(
    _prompt: &Prompt,
    _model_slug: &str,
    agentix_provider_id: &str,
    _api_key: String,
) -> Result<ResponseStream> {
    Err(CodexErr::Stream(
        format!(
            "agentix wire_api is not yet implemented (provider: {agentix_provider_id}); \
             the Prompt → agentix::Request and LlmEvent → ResponseEvent translation \
             layers will land in subsequent commits"
        ),
        None,
    ))
}
