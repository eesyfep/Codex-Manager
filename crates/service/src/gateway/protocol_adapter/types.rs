use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseAdapter {
    Passthrough,
    AnthropicMessagesFromResponses,
    ChatCompletionsFromResponses,
    ResponsesFromChatCompletions,
    ImagesB64JsonFromResponses,
    ImagesUrlFromResponses,
    GeminiJson,
    GeminiSse,
    GeminiCliJson,
    GeminiCliSse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderFamily {
    OpenAI,
    Anthropic,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdapterContractKind {
    NativeResponsesPassthrough,
    ResponsesFromChat,
    ResponsesFromStreamingChat,
    AnthropicNative,
    GeminiNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdapterContract {
    pub(crate) provider_family: ProviderFamily,
    pub(crate) kind: AdapterContractKind,
}

impl AdapterContract {
    pub(crate) const fn native_openai_responses_passthrough() -> Self {
        Self {
            provider_family: ProviderFamily::OpenAI,
            kind: AdapterContractKind::NativeResponsesPassthrough,
        }
    }

    pub(crate) const fn responses_from_chat(provider_family: ProviderFamily) -> Self {
        Self {
            provider_family,
            kind: AdapterContractKind::ResponsesFromChat,
        }
    }

    pub(crate) const fn responses_from_streaming_chat(provider_family: ProviderFamily) -> Self {
        Self {
            provider_family,
            kind: AdapterContractKind::ResponsesFromStreamingChat,
        }
    }

    pub(crate) const fn anthropic_native() -> Self {
        Self {
            provider_family: ProviderFamily::Anthropic,
            kind: AdapterContractKind::AnthropicNative,
        }
    }

    pub(crate) const fn gemini_native() -> Self {
        Self {
            provider_family: ProviderFamily::Gemini,
            kind: AdapterContractKind::GeminiNative,
        }
    }

    pub(crate) const fn response_adapter(self) -> ResponseAdapter {
        match self.kind {
            AdapterContractKind::NativeResponsesPassthrough => ResponseAdapter::Passthrough,
            AdapterContractKind::ResponsesFromChat
            | AdapterContractKind::ResponsesFromStreamingChat => {
                ResponseAdapter::ResponsesFromChatCompletions
            }
            AdapterContractKind::AnthropicNative => ResponseAdapter::AnthropicMessagesFromResponses,
            AdapterContractKind::GeminiNative => ResponseAdapter::GeminiJson,
        }
    }

    pub(crate) const fn upstream_path_for(self, default_path: &str) -> &str {
        match self.kind {
            AdapterContractKind::ResponsesFromChat
            | AdapterContractKind::ResponsesFromStreamingChat => "/v1/chat/completions",
            _ => default_path,
        }
    }

    pub(crate) const fn disables_upstream_stream_passthrough(self) -> bool {
        matches!(self.kind, AdapterContractKind::ResponsesFromChat)
    }

    pub(crate) const fn requires_responses_to_chat_rewrite(self) -> bool {
        matches!(
            self.kind,
            AdapterContractKind::ResponsesFromChat
                | AdapterContractKind::ResponsesFromStreamingChat
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GeminiStreamOutputMode {
    Sse,
    Raw,
}

pub(crate) type ToolNameRestoreMap = BTreeMap<String, String>;

#[derive(Debug)]
pub(crate) struct AdaptedGatewayRequest {
    pub(crate) path: String,
    pub(crate) body: Vec<u8>,
    pub(crate) response_adapter: ResponseAdapter,
    pub(crate) gemini_stream_output_mode: Option<GeminiStreamOutputMode>,
    pub(crate) tool_name_restore_map: ToolNameRestoreMap,
}
