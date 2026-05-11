use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) struct ChatToResponsesLifecycle {
    pub(in super::super) response_id: String,
    pub(in super::super) model: String,
    pub(in super::super) created_at: i64,
    pub(in super::super) item_id: String,
    pub(in super::super) output_text: String,
    pub(in super::super) response: Value,
}

impl ChatToResponsesLifecycle {
    pub(in super::super) fn from_chat_response_body(body: &[u8]) -> Option<Self> {
        let response = chat_completion_response_value(body)?;
        let response_id = response
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("resp_chat")
            .to_string();
        let model = response
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("gpt-5.4")
            .to_string();
        let created_at = response
            .get("created_at")
            .or_else(|| response.get("created"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let output_text = response
            .get("output_text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let item_id = format!("msg_{response_id}");
        Some(Self {
            response_id,
            model,
            created_at,
            item_id,
            output_text,
            response,
        })
    }

    pub(in super::super) fn created_event(&self) -> Value {
        json!({
            "type": "response.created",
            "response": {
                "id": self.response_id,
                "object": "response",
                "created_at": self.created_at,
                "model": self.model,
                "status": "in_progress"
            }
        })
    }

    pub(in super::super) fn in_progress_event(&self) -> Value {
        json!({
            "type": "response.in_progress",
            "response": {
                "id": self.response_id,
                "object": "response",
                "created_at": self.created_at,
                "model": self.model,
                "status": "in_progress"
            }
        })
    }

    pub(in super::super) fn output_item_added_event(&self) -> Value {
        json!({
            "type": "response.output_item.added",
            "response_id": self.response_id,
            "output_index": 0,
            "item": {
                "id": self.item_id,
                "type": "message",
                "status": "in_progress",
                "role": "assistant",
                "content": []
            }
        })
    }

    pub(in super::super) fn content_part_added_event(&self) -> Value {
        json!({
            "type": "response.content_part.added",
            "response_id": self.response_id,
            "item_id": self.item_id,
            "output_index": 0,
            "content_index": 0,
            "part": {
                "type": "output_text",
                "text": ""
            }
        })
    }

    pub(in super::super) fn output_text_delta_event(&self) -> Value {
        json!({
            "type": "response.output_text.delta",
            "response_id": self.response_id,
            "item_id": self.item_id,
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 0,
            "delta": self.output_text
        })
    }

    pub(in super::super) fn output_text_done_event(&self) -> Value {
        json!({
            "type": "response.output_text.done",
            "response_id": self.response_id,
            "item_id": self.item_id,
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 1,
            "text": self.output_text
        })
    }

    pub(in super::super) fn content_part_done_event(&self) -> Value {
        json!({
            "type": "response.content_part.done",
            "response_id": self.response_id,
            "item_id": self.item_id,
            "output_index": 0,
            "content_index": 0,
            "part": {
                "type": "output_text",
                "text": self.output_text
            }
        })
    }

    pub(in super::super) fn output_item_done_event(&self) -> Value {
        json!({
            "type": "response.output_item.done",
            "response_id": self.response_id,
            "output_index": 0,
            "item": {
                "id": self.item_id,
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": self.output_text
                }]
            }
        })
    }

    pub(in super::super) fn completed_event(&self) -> Value {
        json!({
            "type": "response.completed",
            "response": self.response
        })
    }

    pub(in super::super) fn lifecycle_events(&self) -> [(&'static str, Value); 9] {
        [
            ("response.created", self.created_event()),
            ("response.in_progress", self.in_progress_event()),
            ("response.output_item.added", self.output_item_added_event()),
            (
                "response.content_part.added",
                self.content_part_added_event(),
            ),
            ("response.output_text.delta", self.output_text_delta_event()),
            ("response.output_text.done", self.output_text_done_event()),
            ("response.content_part.done", self.content_part_done_event()),
            ("response.output_item.done", self.output_item_done_event()),
            ("response.completed", self.completed_event()),
        ]
    }
}

pub(in super::super) fn lifecycle_sse_bytes(
    contract: &ChatToResponsesLifecycle,
) -> Option<Vec<u8>> {
    let mut out = String::new();
    for (event_name, payload) in contract.lifecycle_events() {
        out.push_str("event: ");
        out.push_str(event_name);
        out.push('\n');
        out.push_str("data: ");
        out.push_str(&serde_json::to_string(&payload).ok()?);
        out.push_str("\n\n");
    }
    Some(out.into_bytes())
}

pub(in super::super) fn chat_completion_response_value(body: &[u8]) -> Option<Value> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let choices = value.get("choices").and_then(Value::as_array)?;
    if choices.is_empty() {
        return None;
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_codexmanager_chat_adapter");
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("gpt-5.4");
    let created = value.get("created").and_then(Value::as_i64).unwrap_or(0);
    let text = chat_completion_text(&value);
    let has_tool_calls = chat_completion_has_tool_calls(&value);
    if text.trim().is_empty() && !has_tool_calls {
        return None;
    }
    let output = if has_tool_calls {
        chat_tool_calls_to_responses_output(&value)
    } else {
        vec![json!({
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text
            }]
        })]
    };
    let mut response = json!({
        "id": id,
        "object": "response",
        "created": created,
        "created_at": created,
        "model": model,
        "status": "completed",
        "output": output,
        "output_text": text
    });
    if let Some(usage) = value.get("usage") {
        response["usage"] = chat_usage_to_responses_usage(usage);
    }
    Some(response)
}

fn collect_chat_output_text(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => out.push_str(text),
        Value::Array(items) => {
            for item in items {
                collect_chat_output_text(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                out.push_str(text);
            } else if let Some(content) = map.get("content") {
                collect_chat_output_text(content, out);
            }
        }
        _ => {}
    }
}

fn chat_usage_to_responses_usage(usage: &Value) -> Value {
    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(input_tokens + output_tokens);
    let mut mapped = json!({
        "input_tokens": input_tokens.max(0),
        "output_tokens": output_tokens.max(0),
        "total_tokens": total_tokens.max(0)
    });
    if let Some(details) = usage
        .get("input_tokens_details")
        .or_else(|| usage.get("prompt_tokens_details"))
    {
        mapped["input_tokens_details"] = details.clone();
    }
    if let Some(details) = usage
        .get("output_tokens_details")
        .or_else(|| usage.get("completion_tokens_details"))
    {
        mapped["output_tokens_details"] = details.clone();
    }
    mapped
}

fn collect_chat_completion_choice_text(choice: &Value) -> String {
    let mut text = String::new();
    if let Some(message) = choice.get("message") {
        if let Some(content) = message.get("content") {
            collect_chat_output_text(content, &mut text);
        }
    }
    if text.is_empty() {
        if let Some(delta) = choice.get("delta") {
            if let Some(content) = delta.get("content") {
                collect_chat_output_text(content, &mut text);
            }
        }
    }
    text
}

fn chat_completion_text(value: &Value) -> String {
    let mut text = String::new();
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            text.push_str(collect_chat_completion_choice_text(choice).as_str());
        }
    }
    text
}

fn chat_completion_has_tool_calls(value: &Value) -> bool {
    value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("message")
                    .and_then(|message| message.get("tool_calls"))
                    .or_else(|| {
                        choice
                            .get("delta")
                            .and_then(|delta| delta.get("tool_calls"))
                    })
                    .and_then(Value::as_array)
                    .is_some_and(|tool_calls| !tool_calls.is_empty())
            })
        })
}

fn chat_tool_calls_to_responses_output(value: &Value) -> Vec<Value> {
    let mut output = Vec::new();
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return output;
    };
    for choice in choices {
        let tool_calls = choice
            .get("message")
            .and_then(|message| message.get("tool_calls"))
            .or_else(|| {
                choice
                    .get("delta")
                    .and_then(|delta| delta.get("tool_calls"))
            })
            .and_then(Value::as_array);
        let Some(tool_calls) = tool_calls else {
            continue;
        };
        for (idx, call) in tool_calls.iter().enumerate() {
            let function = call.get("function").unwrap_or(call);
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string();
            let call_id = call
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("call_chat_{idx}"));
            output.push(json!({
                "type": "function_call",
                "id": format!("fc_{call_id}"),
                "call_id": call_id,
                "name": name,
                "arguments": arguments
            }));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{lifecycle_sse_bytes, ChatToResponsesLifecycle};
    use serde_json::json;

    #[test]
    fn lifecycle_contract_emits_expected_event_order() {
        let body = json!({
            "id": "chatcmpl_contract_1",
            "created": 1775900200,
            "model": "glm-5.1",
            "choices": [{
                "delta": { "content": "hello contract" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 2,
                "completion_tokens": 3,
                "total_tokens": 5
            }
        });
        let contract = ChatToResponsesLifecycle::from_chat_response_body(
            serde_json::to_vec(&body).expect("body").as_slice(),
        )
        .expect("contract");
        let sse = String::from_utf8(lifecycle_sse_bytes(&contract).expect("sse")).expect("utf8");
        let order = [
            "event: response.created",
            "event: response.in_progress",
            "event: response.output_item.added",
            "event: response.content_part.added",
            "event: response.output_text.delta",
            "event: response.output_text.done",
            "event: response.content_part.done",
            "event: response.output_item.done",
            "event: response.completed",
        ];
        let mut last_index = 0usize;
        for marker in order {
            let idx = sse.find(marker).expect("marker present");
            assert!(idx >= last_index, "marker {marker} out of order");
            last_index = idx;
        }
        assert!(sse.contains("\"item_id\":\"msg_chatcmpl_contract_1\""));
        assert!(sse.contains("\"output_text\":\"hello contract\""));
        assert!(sse.contains("\"input_tokens\":2"));
        assert!(sse.contains("\"output_tokens\":3"));
        assert!(sse.contains("\"total_tokens\":5"));
    }

    #[test]
    fn chat_completion_response_value_rejects_empty_success_body() {
        let body = json!({
            "id": "chatcmpl_empty",
            "created": 1775900201,
            "model": "glm-5.1",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": ""
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 0,
                "total_tokens": 1
            }
        });

        assert!(super::chat_completion_response_value(
            serde_json::to_vec(&body).expect("body").as_slice()
        )
        .is_none());
    }

    #[test]
    fn chat_completion_tool_calls_become_responses_function_calls() {
        let body = json!({
            "id": "chatcmpl_tool_1",
            "created": 1775900202,
            "model": "glm-5.1",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_read_file_1",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"README.md\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 4,
                "completion_tokens": 3,
                "total_tokens": 7
            }
        });

        let response = super::chat_completion_response_value(
            serde_json::to_vec(&body).expect("body").as_slice(),
        )
        .expect("response");
        assert_eq!(response["output"][0]["type"], "function_call");
        assert_eq!(response["output"][0]["call_id"], "call_read_file_1");
        assert_eq!(response["output"][0]["name"], "read_file");
        assert_eq!(
            response["output"][0]["arguments"],
            "{\"path\":\"README.md\"}"
        );
        assert_eq!(response["usage"]["input_tokens"], 4);
        assert_eq!(response["usage"]["output_tokens"], 3);
    }
}
