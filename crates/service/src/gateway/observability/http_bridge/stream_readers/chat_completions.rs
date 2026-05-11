use super::{
    chat_image_payload, classify_upstream_stream_read_error, collect_image_generation_data_urls,
    collect_response_output_text, mark_first_response_ms, merge_usage, should_emit_keepalive,
    stream_idle_timed_out, stream_idle_timeout_message, stream_reader_disconnected_message,
    stream_wait_timeout, upstream_hint_or_stream_incomplete_message, Arc, Cursor, Mutex,
    PassthroughSseCollector, Read, SseKeepAliveFrame, UpstreamSseFramePump,
    UpstreamSseFramePumpItem,
};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

pub(crate) struct ChatCompletionsFromResponsesSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    request_started_at: Instant,
    last_upstream_activity: Instant,
    saw_upstream_frame: bool,
    finished: bool,
    emitted_text: bool,
    emitted_image_urls: HashSet<String>,
    pending_tool_calls: BTreeMap<i64, PendingResponsesToolCall>,
    id: Option<String>,
    model: Option<String>,
    created: Option<i64>,
}

#[derive(Default, Clone)]
struct PendingResponsesToolCall {
    id: Option<String>,
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
    emitted: bool,
}

impl ChatCompletionsFromResponsesSseReader {
    pub(crate) fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        request_started_at: Instant,
    ) -> Self {
        Self {
            upstream: UpstreamSseFramePump::new(upstream),
            out_cursor: Cursor::new(Vec::new()),
            usage_collector,
            request_started_at,
            last_upstream_activity: Instant::now(),
            saw_upstream_frame: false,
            finished: false,
            emitted_text: false,
            emitted_image_urls: HashSet::new(),
            pending_tool_calls: BTreeMap::new(),
            id: None,
            model: None,
            created: None,
        }
    }

    fn data_json(lines: &[String]) -> Option<Value> {
        let mut data = String::new();
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = trimmed.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start());
            }
        }
        if data.is_empty() || data.trim() == "[DONE]" {
            return None;
        }
        serde_json::from_str(data.as_str()).ok()
    }

    fn event_type(lines: &[String], value: &Value) -> Option<String> {
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']).trim_start();
            if let Some(rest) = trimmed.strip_prefix("event:") {
                let event = rest.trim();
                if !event.is_empty() {
                    return Some(event.to_string());
                }
            }
        }
        value
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn remember_meta(&mut self, value: &Value) {
        let response = value.get("response");
        if self.id.is_none() {
            self.id = response
                .and_then(|v| v.get("id"))
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if self.model.is_none() {
            self.model = response
                .and_then(|v| v.get("model"))
                .or_else(|| value.get("model"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if self.created.is_none() {
            self.created = response
                .and_then(|v| v.get("created_at"))
                .or_else(|| response.and_then(|v| v.get("created")))
                .or_else(|| value.get("created_at"))
                .or_else(|| value.get("created"))
                .and_then(Value::as_i64);
        }
        if let Some(usage) = response
            .and_then(|v| v.get("usage"))
            .or_else(|| value.get("usage"))
            .cloned()
        {
            if let Ok(mut collector) = self.usage_collector.lock() {
                merge_usage(
                    &mut collector.usage,
                    super::super::parse_usage_from_json(&serde_json::json!({ "usage": usage })),
                );
            }
        }
    }

    fn chat_id(&self) -> String {
        self.id
            .clone()
            .unwrap_or_else(|| "chatcmpl_codexmanager".to_string())
    }

    fn chat_model(&self) -> String {
        self.model.clone().unwrap_or_else(|| "gpt-5.4".to_string())
    }

    fn chat_created(&self) -> i64 {
        self.created.unwrap_or(0)
    }

    fn chunk(&self, delta: Value, finish_reason: Option<&str>, usage: Option<Value>) -> Vec<u8> {
        let mut chunk = serde_json::json!({
            "id": self.chat_id(),
            "object": "chat.completion.chunk",
            "created": self.chat_created(),
            "model": self.chat_model(),
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason
            }]
        });
        if let Some(usage) = usage {
            chunk["usage"] = usage;
        }
        format!(
            "data: {}\n\n",
            serde_json::to_string(&chunk).unwrap_or_else(|_| "{}".to_string())
        )
        .into_bytes()
    }

    fn final_chunk(&self) -> Vec<u8> {
        let usage = self.usage_collector.lock().ok().map(|collector| {
            serde_json::json!({
                "prompt_tokens": collector.usage.input_tokens.unwrap_or(0),
                "completion_tokens": collector.usage.output_tokens.unwrap_or(0),
                "total_tokens": collector.usage.total_tokens.unwrap_or(0)
            })
        });
        let mut out = self.chunk(serde_json::json!({}), Some("stop"), usage);
        out.extend_from_slice(b"data: [DONE]\n\n");
        out
    }

    fn done_only_chunk(&self) -> Vec<u8> {
        b"data: [DONE]\n\n".to_vec()
    }

    fn image_delta_chunk(&mut self, value: &Value) -> Option<Vec<u8>> {
        let images = collect_image_generation_data_urls(value)
            .into_iter()
            .filter(|url| self.emitted_image_urls.insert(url.clone()))
            .enumerate()
            .map(|(index, url)| chat_image_payload(url, index))
            .collect::<Vec<_>>();
        if images.is_empty() {
            None
        } else {
            Some(self.chunk(
                serde_json::json!({
                    "role": "assistant",
                    "images": images
                }),
                None,
                None,
            ))
        }
    }

    fn update_terminal_success(&self, event_type: Option<&str>) {
        if let Ok(mut collector) = self.usage_collector.lock() {
            if let Some(event_type) = event_type {
                collector.last_event_type = Some(event_type.to_string());
            }
            collector.saw_terminal = true;
        }
    }

    fn has_recoverable_partial_output(&self) -> bool {
        self.emitted_text
            || !self.emitted_image_urls.is_empty()
            || self
                .pending_tool_calls
                .values()
                .any(|call| call.name.is_some() && has_meaningful_tool_arguments(&call.arguments))
    }

    fn merge_function_call_item(
        &mut self,
        output_index: i64,
        item: &serde_json::Map<String, Value>,
    ) {
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            return;
        }
        let entry = self.pending_tool_calls.entry(output_index).or_default();
        if entry.id.is_none() {
            entry.id = item.get("id").and_then(Value::as_str).map(str::to_string);
        }
        if entry.call_id.is_none() {
            entry.call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if entry.name.is_none() {
            entry.name = item.get("name").and_then(Value::as_str).map(str::to_string);
        }
        if let Some(arguments) = item.get("arguments").and_then(Value::as_str) {
            merge_tool_call_arguments(&mut entry.arguments, arguments);
        }
    }

    fn merge_function_call_event(&mut self, value: &Value) {
        let Some(event_type) = Self::event_type(&[], value) else {
            return;
        };
        match event_type.as_str() {
            "response.output_item.added" | "response.output_item.done" => {
                let output_index = value
                    .get("output_index")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                if let Some(item) = value.get("item").and_then(Value::as_object) {
                    self.merge_function_call_item(output_index, item);
                }
            }
            "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
                let output_index = value
                    .get("output_index")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let entry = self.pending_tool_calls.entry(output_index).or_default();
                if entry.id.is_none() {
                    entry.id = value
                        .get("item_id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                if entry.call_id.is_none() {
                    entry.call_id = value
                        .get("call_id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                if let Some(arguments) = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .or_else(|| value.get("arguments").and_then(Value::as_str))
                {
                    merge_tool_call_arguments(&mut entry.arguments, arguments);
                }
            }
            _ => {}
        }
    }

    fn merge_completed_response_tool_calls(&mut self, value: &Value) {
        let Some(output) = value
            .get("response")
            .and_then(|response| response.get("output"))
            .and_then(Value::as_array)
        else {
            return;
        };
        for (output_index, item) in output.iter().enumerate() {
            let Some(item_obj) = item.as_object() else {
                continue;
            };
            self.merge_function_call_item(output_index as i64, item_obj);
        }
    }

    fn emit_pending_tool_call_chunks(&mut self, finish_reason: Option<&str>) -> Vec<u8> {
        let mut out = Vec::new();
        let mut deltas = Vec::new();
        for (output_index, call) in &mut self.pending_tool_calls {
            if call.emitted {
                continue;
            }
            let Some(name) = call.name.as_deref() else {
                continue;
            };
            let Some(call_id) = call.call_id.as_deref().or(call.id.as_deref()) else {
                continue;
            };
            if !has_meaningful_tool_arguments(&call.arguments) {
                continue;
            }
            call.emitted = true;
            deltas.push(serde_json::json!({
                "tool_calls": [{
                    "index": output_index,
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": call.arguments
                    }
                }]
            }));
        }
        for delta in deltas {
            out.extend(self.chunk(delta, finish_reason, None));
        }
        out
    }

    fn synthesize_completion_from_partial_output(
        &mut self,
        event_type: Option<&str>,
        reason: &str,
    ) -> Option<std::io::Result<Vec<u8>>> {
        if self.finished || !self.has_recoverable_partial_output() {
            return None;
        }
        log::warn!(
            "event=chat_from_responses_partial_completion model={} response_id={} emitted_text={} emitted_images={} pending_tool_calls={} reason={}",
            self.chat_model(),
            self.chat_id(),
            self.emitted_text,
            self.emitted_image_urls.len(),
            self.pending_tool_calls.len(),
            reason
        );
        self.update_terminal_success(event_type);
        self.finished = true;
        let mut out = self.emit_pending_tool_call_chunks(Some("tool_calls"));
        if out.is_empty() {
            out = self.final_chunk();
        } else {
            out.extend_from_slice(&self.done_only_chunk());
        }
        Some(Ok(out))
    }

    fn handle_frame(&mut self, lines: &[String]) -> Option<Vec<u8>> {
        let value = Self::data_json(lines)?;
        self.remember_meta(&value);
        let event_type = Self::event_type(lines, &value);
        self.merge_function_call_event(&value);
        let mut text = String::new();
        if matches!(
            event_type.as_deref(),
            Some("response.output_text.delta")
                | Some("response.output_text.done")
                | Some("response.content_part.delta")
                | Some("response.content_part.done")
        ) {
            if let Some(delta) = value.get("delta") {
                collect_response_output_text(delta, &mut text);
            }
        }
        if matches!(
            event_type.as_deref(),
            Some("response.completed") | Some("response.done")
        ) {
            let mut out = Vec::new();
            self.merge_completed_response_tool_calls(&value);
            if !self.emitted_text {
                if let Some(response) = value.get("response") {
                    collect_response_output_text(response, &mut text);
                }
                if !text.is_empty() {
                    out.extend(self.chunk(serde_json::json!({ "content": text }), None, None));
                    self.emitted_text = true;
                }
            }
            if let Some(response) = value.get("response") {
                if let Some(images) = self.image_delta_chunk(response) {
                    out.extend(images);
                }
            }
            let tool_chunks = self.emit_pending_tool_call_chunks(Some("tool_calls"));
            let emitted_tool_chunks = !tool_chunks.is_empty();
            out.extend(tool_chunks);
            self.update_terminal_success(event_type.as_deref());
            self.finished = true;
            if emitted_tool_chunks && !self.emitted_text && self.emitted_image_urls.is_empty() {
                out.extend_from_slice(&self.done_only_chunk());
            } else {
                out.extend(self.final_chunk());
            }
            return Some(out);
        }
        if event_type.as_deref() == Some("response.output_item.done") {
            let tool_chunks = self.emit_pending_tool_call_chunks(Some("tool_calls"));
            if !tool_chunks.is_empty() {
                return Some(tool_chunks);
            }
            if let Some(images) = self.image_delta_chunk(&value) {
                return Some(images);
            }
        }
        if event_type.as_deref() == Some("response.function_call_arguments.done") {
            let tool_chunks = self.emit_pending_tool_call_chunks(Some("tool_calls"));
            if !tool_chunks.is_empty() {
                return Some(tool_chunks);
            }
        }
        if event_type.as_deref() == Some("response.image_generation_call.partial_image") {
            if let Some(images) = self.image_delta_chunk(&value) {
                return Some(images);
            }
        }
        if text.is_empty() {
            if let Some(response) = value.get("response") {
                collect_response_output_text(response, &mut text);
            }
        }
        if !text.is_empty() {
            self.emitted_text = true;
            return Some(self.chunk(serde_json::json!({ "content": text }), None, None));
        }
        None
    }

    fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        loop {
            match self
                .upstream
                .recv_timeout(stream_wait_timeout(self.last_upstream_activity))
            {
                Ok(UpstreamSseFramePumpItem::Frame(frame)) => {
                    self.last_upstream_activity = Instant::now();
                    self.saw_upstream_frame = true;
                    mark_first_response_ms(&self.usage_collector, self.request_started_at);
                    if let Some(chunk) = self.handle_frame(&frame) {
                        return Ok(chunk);
                    }
                    continue;
                }
                Ok(UpstreamSseFramePumpItem::Eof) => {
                    if let Some(result) =
                        self.synthesize_completion_from_partial_output(None, "upstream_eof")
                    {
                        return result;
                    }
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        if !collector.saw_terminal {
                            let hint = collector.upstream_error_hint.clone();
                            collector.terminal_error.get_or_insert_with(|| {
                                upstream_hint_or_stream_incomplete_message(hint.as_deref())
                            });
                        }
                    }
                    self.finished = true;
                    return Ok(Vec::new());
                }
                Ok(UpstreamSseFramePumpItem::Error(err)) => {
                    if let Some(result) =
                        self.synthesize_completion_from_partial_output(None, "upstream_error")
                    {
                        return result;
                    }
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        collector
                            .terminal_error
                            .get_or_insert_with(|| classify_upstream_stream_read_error(&err));
                    }
                    self.finished = true;
                    return Ok(Vec::new());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if stream_idle_timed_out(self.last_upstream_activity) {
                        if let Some(result) =
                            self.synthesize_completion_from_partial_output(None, "idle_timeout")
                        {
                            return result;
                        }
                        if let Ok(mut collector) = self.usage_collector.lock() {
                            collector
                                .terminal_error
                                .get_or_insert_with(stream_idle_timeout_message);
                        }
                        self.finished = true;
                        return Ok(Vec::new());
                    }
                    if should_emit_keepalive(self.saw_upstream_frame) {
                        return Ok(SseKeepAliveFrame::Comment.bytes().to_vec());
                    }
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    if let Some(result) =
                        self.synthesize_completion_from_partial_output(None, "reader_disconnected")
                    {
                        return result;
                    }
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        let hint = collector.upstream_error_hint.clone();
                        collector.terminal_error.get_or_insert_with(|| {
                            hint.unwrap_or_else(stream_reader_disconnected_message)
                        });
                    }
                    self.finished = true;
                    return Ok(Vec::new());
                }
            }
        }
    }
}

impl Read for ChatCompletionsFromResponsesSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.out_cursor.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
            if self.finished {
                return Ok(0);
            }
            self.out_cursor = Cursor::new(self.next_chunk()?);
        }
    }
}

pub(crate) struct ResponsesFromChatCompletionsSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    request_started_at: Instant,
    last_upstream_activity: Instant,
    saw_upstream_frame: bool,
    finished: bool,
    created_sent: bool,
    message_item_opened: bool,
    response_id: Option<String>,
    model: Option<String>,
    created: Option<i64>,
    output_text: String,
    sequence_number: i64,
    pending_tool_calls: BTreeMap<i64, PendingChatToolCall>,
    emitted_tool_calls: BTreeMap<i64, String>,
    handshake_sent: bool,
}

#[derive(Default, Clone)]
struct PendingChatToolCall {
    id: Option<String>,
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
    emitted_added: bool,
    emitted_argument_len: usize,
    emitted_done: bool,
}

fn merge_tool_call_arguments(existing: &mut String, fragment: &str) {
    let trimmed = fragment.trim();
    if trimmed.is_empty() {
        return;
    }
    if existing.is_empty() {
        existing.push_str(trimmed);
        return;
    }
    if existing == trimmed || existing.ends_with(trimmed) || existing.starts_with(trimmed) {
        return;
    }
    if trimmed.starts_with(existing.as_str()) {
        *existing = trimmed.to_string();
        return;
    }
    if should_replace_tool_call_arguments(existing, trimmed) {
        *existing = trimmed.to_string();
        return;
    }
    existing.push_str(trimmed);
}

fn parse_json_object_lenient(raw: &str) -> Option<Value> {
    let mut current = raw.trim().to_string();
    for _ in 0..3 {
        let parsed = serde_json::from_str::<Value>(&current).ok()?;
        match parsed {
            Value::Object(_) => return Some(parsed),
            Value::String(inner) => {
                let trimmed = inner.trim();
                if trimmed.is_empty() || trimmed == current {
                    return None;
                }
                current = trimmed.to_string();
            }
            _ => return None,
        }
    }
    None
}

fn has_meaningful_tool_arguments(raw: &str) -> bool {
    parse_json_object_lenient(raw)
        .and_then(|value| value.as_object().map(|obj| !obj.is_empty()))
        .unwrap_or(false)
}

fn should_replace_tool_call_arguments(existing: &str, candidate: &str) -> bool {
    let existing_len = parse_json_object_lenient(existing)
        .and_then(|value| value.as_object().map(|obj| obj.len()));
    let candidate_len = parse_json_object_lenient(candidate)
        .and_then(|value| value.as_object().map(|obj| obj.len()));
    match (existing_len, candidate_len) {
        (None, Some(_)) => true,
        (Some(current), Some(next)) => next >= current && candidate.len() >= existing.len(),
        _ => false,
    }
}

struct GatewayByteStreamReadAdapter {
    upstream: crate::gateway::upstream::GatewayByteStream,
    pending: Cursor<Vec<u8>>,
    finished: bool,
}

impl GatewayByteStreamReadAdapter {
    fn new(upstream: crate::gateway::upstream::GatewayByteStream) -> Self {
        Self {
            upstream,
            pending: Cursor::new(Vec::new()),
            finished: false,
        }
    }
}

impl Read for GatewayByteStreamReadAdapter {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.pending.read(buf)?;
            if read > 0 || self.finished {
                return Ok(read);
            }
            match self.upstream.recv() {
                Ok(crate::gateway::upstream::GatewayByteStreamItem::Chunk(bytes)) => {
                    self.pending = Cursor::new(bytes.to_vec());
                }
                Ok(crate::gateway::upstream::GatewayByteStreamItem::Eof) => {
                    self.finished = true;
                    return Ok(0);
                }
                Ok(crate::gateway::upstream::GatewayByteStreamItem::Error(err)) => {
                    self.finished = true;
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, err));
                }
                Err(_) => {
                    self.finished = true;
                    return Ok(0);
                }
            }
        }
    }
}

impl ResponsesFromChatCompletionsSseReader {
    pub(crate) fn from_reader<R>(
        upstream: R,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        request_started_at: Instant,
    ) -> Self
    where
        R: Read + Send + 'static,
    {
        Self {
            upstream: UpstreamSseFramePump::from_reader(upstream),
            out_cursor: Cursor::new(Vec::new()),
            usage_collector,
            request_started_at,
            last_upstream_activity: Instant::now(),
            saw_upstream_frame: false,
            finished: false,
            created_sent: false,
            message_item_opened: false,
            response_id: None,
            model: None,
            created: None,
            output_text: String::new(),
            sequence_number: 0,
            pending_tool_calls: BTreeMap::new(),
            emitted_tool_calls: BTreeMap::new(),
            handshake_sent: false,
        }
    }

    pub(crate) fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        request_started_at: Instant,
    ) -> Self {
        Self::from_reader(upstream, usage_collector, request_started_at)
    }

    pub(crate) fn from_gateway_stream(
        upstream: crate::gateway::upstream::GatewayByteStream,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        request_started_at: Instant,
    ) -> Self {
        Self::from_reader(
            GatewayByteStreamReadAdapter::new(upstream),
            usage_collector,
            request_started_at,
        )
    }

    fn data_text(lines: &[String]) -> Option<String> {
        let mut data = String::new();
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = trimmed.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start());
            }
        }
        (!data.is_empty()).then_some(data)
    }

    fn data_json(lines: &[String]) -> Option<Value> {
        let data = Self::data_text(lines)?;
        if data.trim() == "[DONE]" {
            return None;
        }
        serde_json::from_str(data.as_str()).ok()
    }

    fn remember_meta(&mut self, value: &Value) {
        if !self.handshake_sent {
            if self.response_id.is_none() {
                self.response_id = value.get("id").and_then(Value::as_str).map(str::to_string);
            }
            if self.model.is_none() {
                self.model = value
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            if self.created.is_none() {
                self.created = value.get("created").and_then(Value::as_i64);
            }
        }
        if let Ok(mut collector) = self.usage_collector.lock() {
            merge_usage(
                &mut collector.usage,
                super::super::parse_usage_from_json(value),
            );
        }
    }

    fn response_id(&self) -> String {
        self.response_id
            .clone()
            .unwrap_or_else(|| "resp_codexmanager_chat_adapter".to_string())
    }

    fn model(&self) -> String {
        self.model.clone().unwrap_or_else(|| "gpt-5.4".to_string())
    }

    fn created(&self) -> i64 {
        self.created.unwrap_or(0)
    }

    fn item_id(&self) -> String {
        format!("msg_{}", self.response_id())
    }

    fn next_sequence_number(&mut self) -> i64 {
        let sequence_number = self.sequence_number;
        self.sequence_number += 1;
        sequence_number
    }

    fn sse_event(event: &str, payload: Value) -> Vec<u8> {
        format!(
            "event: {event}\ndata: {}\n\n",
            serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
        )
        .into_bytes()
    }

    fn created_event(&self) -> Vec<u8> {
        Self::sse_event(
            "response.created",
            serde_json::json!({
                "type": "response.created",
                "response": {
                    "id": self.response_id(),
                    "object": "response",
                    "created_at": self.created(),
                    "model": self.model(),
                    "status": "in_progress"
                }
            }),
        )
    }

    fn in_progress_event(&self) -> Vec<u8> {
        Self::sse_event(
            "response.in_progress",
            serde_json::json!({
                "type": "response.in_progress",
                "response": {
                    "id": self.response_id(),
                    "object": "response",
                    "created_at": self.created(),
                    "model": self.model(),
                    "status": "in_progress"
                }
            }),
        )
    }

    fn handshake_events(&mut self) -> Vec<u8> {
        if self.handshake_sent {
            return Vec::new();
        }
        self.response_id
            .get_or_insert_with(|| "resp_codexmanager_chat_adapter".to_string());
        self.model.get_or_insert_with(|| "gpt-5.4".to_string());
        self.created.get_or_insert(0);
        self.handshake_sent = true;
        self.created_sent = true;
        let mut out = self.created_event();
        out.extend(self.in_progress_event());
        out
    }

    fn delta_event(&mut self, delta: &str) -> Vec<u8> {
        let sequence_number = self.next_sequence_number();
        Self::sse_event(
            "response.output_text.delta",
            serde_json::json!({
                "type": "response.output_text.delta",
                "response_id": self.response_id(),
                "item_id": self.item_id(),
                "output_index": 0,
                "content_index": 0,
                "sequence_number": sequence_number,
                "delta": delta
            }),
        )
    }

    fn usage_value(&self) -> Value {
        let usage = self
            .usage_collector
            .lock()
            .map(|collector| collector.usage.clone())
            .unwrap_or_default();
        serde_json::json!({
            "input_tokens": usage.input_tokens.unwrap_or(0),
            "output_tokens": usage.output_tokens.unwrap_or(0),
            "total_tokens": usage.total_tokens.unwrap_or_else(|| {
                usage.input_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0)
            })
        })
    }

    fn completed_event(&self) -> Vec<u8> {
        let mut output_items = Vec::new();
        if !self.output_text.is_empty() {
            output_items.push(serde_json::json!({
                "id": self.item_id(),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": self.output_text,
                    "annotations": []
                }]
            }));
        }
        for (output_index, call) in &self.pending_tool_calls {
            let Some(name) = call.name.as_deref() else {
                continue;
            };
            let Some(call_id) = call.call_id.as_deref().or(call.id.as_deref()) else {
                continue;
            };
            if !has_meaningful_tool_arguments(&call.arguments) {
                continue;
            }
            output_items.push(serde_json::json!({
                "id": call.id.clone().unwrap_or_else(|| format!("fc_{}_{}", self.response_id(), output_index)),
                "type": "function_call",
                "call_id": call_id,
                "name": name,
                "arguments": call.arguments
            }));
        }
        let response = serde_json::json!({
            "id": self.response_id(),
            "object": "response",
            "created_at": self.created(),
            "created": self.created(),
            "model": self.model(),
            "status": "completed",
            "output": output_items,
            "output_text": self.output_text,
            "usage": self.usage_value()
        });
        let contract = super::super::ChatToResponsesLifecycle {
            response_id: self.response_id(),
            model: self.model(),
            created_at: self.created(),
            item_id: self.item_id(),
            output_text: self.output_text.clone(),
            response,
        };
        let mut out = Vec::new();
        if self.message_item_opened {
            out.extend(Self::sse_event(
                "response.output_text.done",
                contract.output_text_done_event(),
            ));
            out.extend(Self::sse_event(
                "response.content_part.done",
                contract.content_part_done_event(),
            ));
            out.extend(Self::sse_event(
                "response.output_item.done",
                contract.output_item_done_event(),
            ));
        }
        out.extend(Self::sse_event(
            "response.completed",
            contract.completed_event(),
        ));
        out
    }

    fn terminal_error_event(&self, message: &str) -> Vec<u8> {
        Self::sse_event(
            "response.failed",
            serde_json::json!({
                "type": "response.failed",
                "response": {
                    "id": self.response_id(),
                    "object": "response",
                    "created_at": self.created(),
                    "created": self.created(),
                    "model": self.model(),
                    "status": "failed",
                    "error": {
                        "message": message,
                        "type": "upstream_error",
                        "code": "upstream_error"
                    }
                }
            }),
        )
    }

    fn update_terminal_success(&self) {
        if let Ok(mut collector) = self.usage_collector.lock() {
            collector.last_event_type = Some("response.completed".to_string());
            collector.saw_terminal = true;
        }
    }

    fn mark_terminal_error(&self, message: String) {
        if let Ok(mut collector) = self.usage_collector.lock() {
            collector.last_event_type = Some("response.failed".to_string());
            collector.upstream_error_hint.get_or_insert(message.clone());
            collector.terminal_error.get_or_insert(message);
        }
    }

    fn has_recoverable_partial_output(&self) -> bool {
        !self.output_text.trim().is_empty() || !self.pending_tool_calls.is_empty()
    }

    fn build_empty_success_message(&self) -> String {
        format!(
            "invalid upstream chat completion response: no assistant output or tool call for model {}",
            self.model()
        )
    }

    fn synthesize_completion_from_partial_output(
        &mut self,
        reason: &str,
    ) -> Option<std::io::Result<Vec<u8>>> {
        if self.finished || !self.has_recoverable_partial_output() {
            return None;
        }
        self.finished = true;
        self.update_terminal_success();
        let pending_indices = self.pending_tool_call_indices();
        let mut out = Vec::new();
        if !pending_indices.is_empty() {
            out.extend(self.emit_tool_call_done_events(&pending_indices));
        }
        log::warn!(
            "event=responses_from_chat_partial_completion model={} response_id={} output_text_len={} pending_tool_calls={} reason={}",
            self.model(),
            self.response_id(),
            self.output_text.len(),
            self.pending_tool_calls.len(),
            reason
        );
        out.extend(self.completed_event());
        Some(Ok(out))
    }

    fn choice_delta_text(value: &Value) -> String {
        let mut out = String::new();
        if let Some(choices) = value.get("choices").and_then(Value::as_array) {
            for choice in choices {
                if let Some(content) = choice.get("delta").and_then(|delta| delta.get("content")) {
                    collect_response_output_text(content, &mut out);
                }
                if out.is_empty() {
                    if let Some(content) = choice
                        .get("message")
                        .and_then(|message| message.get("content"))
                    {
                        collect_response_output_text(content, &mut out);
                    }
                }
            }
        }
        out
    }

    fn merge_tool_call_delta(&mut self, value: &Value) {
        let Some(choices) = value.get("choices").and_then(Value::as_array) else {
            return;
        };
        for choice in choices {
            let tool_calls = choice
                .get("delta")
                .and_then(|delta| delta.get("tool_calls"))
                .or_else(|| choice.get("message").and_then(|msg| msg.get("tool_calls")))
                .and_then(Value::as_array);
            let Some(tool_calls) = tool_calls else {
                continue;
            };
            for tool_call in tool_calls {
                let Some(tool_obj) = tool_call.as_object() else {
                    continue;
                };
                let output_index = tool_obj.get("index").and_then(Value::as_i64).unwrap_or(0);
                let entry = self.pending_tool_calls.entry(output_index).or_default();
                if entry.id.is_none() {
                    entry.id = tool_obj
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                let function_obj = tool_obj.get("function").and_then(Value::as_object);
                if entry.name.is_none() {
                    entry.name = function_obj
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|current| !current.is_empty())
                        .map(str::to_string);
                }
                if let Some(call_id) = tool_obj
                    .get("call_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|current| !current.is_empty())
                {
                    entry.call_id = Some(call_id.to_string());
                }
                if let Some(arguments) = function_obj
                    .and_then(|function| function.get("arguments"))
                    .and_then(Value::as_str)
                {
                    merge_tool_call_arguments(&mut entry.arguments, arguments);
                }
            }
        }
    }

    fn emit_tool_call_events(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let response_id = self.response_id();
        for (output_index, call) in &mut self.pending_tool_calls {
            if !has_meaningful_tool_arguments(&call.arguments) {
                continue;
            }
            let Some(name) = call.name.as_deref() else {
                continue;
            };
            let Some(call_id) = call.call_id.as_deref().or(call.id.as_deref()) else {
                continue;
            };
            let item_id = call
                .id
                .clone()
                .unwrap_or_else(|| format!("fc_{}_{}", response_id, output_index));
            if !call.emitted_added {
                call.emitted_added = true;
                out.extend(Self::sse_event(
                    "response.output_item.added",
                    serde_json::json!({
                        "type": "response.output_item.added",
                        "response_id": response_id,
                        "output_index": output_index,
                        "item": {
                            "type": "function_call",
                            "id": item_id,
                            "call_id": call_id,
                            "name": name,
                            "arguments": ""
                        }
                    }),
                ));
            }
            if call.arguments.len() > call.emitted_argument_len {
                let delta = call.arguments[call.emitted_argument_len..].to_string();
                call.emitted_argument_len = call.arguments.len();
                if !delta.is_empty() {
                    out.extend(Self::sse_event(
                        "response.function_call_arguments.delta",
                        serde_json::json!({
                            "type": "response.function_call_arguments.delta",
                            "response_id": response_id,
                            "item_id": item_id,
                            "output_index": output_index,
                            "call_id": call_id,
                            "delta": delta
                        }),
                    ));
                }
            }
        }
        out
    }

    fn tool_call_terminal_indices(value: &Value) -> Vec<i64> {
        let mut out = Vec::new();
        let Some(choices) = value.get("choices").and_then(Value::as_array) else {
            return out;
        };
        for choice in choices {
            let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
            if !matches!(finish_reason, Some("tool_calls") | Some("function_call")) {
                continue;
            }
            if let Some(tool_calls) = choice
                .get("delta")
                .and_then(|delta| delta.get("tool_calls"))
                .or_else(|| choice.get("message").and_then(|msg| msg.get("tool_calls")))
                .and_then(Value::as_array)
            {
                for tool_call in tool_calls {
                    if let Some(index) = tool_call.get("index").and_then(Value::as_i64) {
                        out.push(index);
                    }
                }
            } else {
                out.push(0);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn emit_tool_call_done_events(&mut self, output_indices: &[i64]) -> Vec<u8> {
        let mut out = Vec::new();
        let response_id = self.response_id();
        for output_index in output_indices {
            let Some(call) = self.pending_tool_calls.get_mut(output_index) else {
                continue;
            };
            if !has_meaningful_tool_arguments(&call.arguments) {
                continue;
            }
            let Some(name) = call.name.as_deref() else {
                continue;
            };
            let Some(call_id) = call.call_id.as_deref().or(call.id.as_deref()) else {
                continue;
            };
            if call.emitted_done {
                continue;
            }
            let item_id = call
                .id
                .clone()
                .unwrap_or_else(|| format!("fc_{}_{}", response_id, output_index));
            call.emitted_done = true;
            self.emitted_tool_calls
                .insert(*output_index, call_id.to_string());
            out.extend(Self::sse_event(
                "response.function_call_arguments.done",
                serde_json::json!({
                    "type": "response.function_call_arguments.done",
                    "response_id": response_id,
                    "output_index": output_index,
                    "item_id": item_id,
                    "call_id": call_id,
                    "arguments": call.arguments
                }),
            ));
            out.extend(Self::sse_event(
                "response.output_item.done",
                serde_json::json!({
                    "type": "response.output_item.done",
                    "response_id": response_id,
                    "output_index": output_index,
                    "item": {
                        "type": "function_call",
                        "id": item_id,
                        "call_id": call_id,
                        "name": name,
                        "arguments": call.arguments
                    }
                }),
            ));
        }
        out
    }

    fn pending_tool_call_indices(&self) -> Vec<i64> {
        self.pending_tool_calls
            .iter()
            .filter_map(|(output_index, call)| {
                if call.emitted_done {
                    None
                } else if call.name.is_some()
                    && (call.call_id.is_some() || call.id.is_some())
                    && has_meaningful_tool_arguments(&call.arguments)
                {
                    Some(*output_index)
                } else {
                    None
                }
            })
            .collect()
    }

    fn handle_frame(&mut self, lines: &[String]) -> Option<Vec<u8>> {
        if Self::data_text(lines)
            .as_deref()
            .is_some_and(|data| data.trim() == "[DONE]")
        {
            if !self.finished {
                let pending_indices = self.pending_tool_call_indices();
                if !pending_indices.is_empty() {
                    let mut out = self.emit_tool_call_done_events(&pending_indices);
                    self.finished = true;
                    self.update_terminal_success();
                    log::warn!(
                        "event=responses_from_chat_done_flush_pending model={} response_id={} saw_frame=true output_text_len={} pending_tool_calls={}",
                        self.model(),
                        self.response_id(),
                        self.output_text.len(),
                        self.pending_tool_calls.len()
                    );
                    out.extend(self.completed_event());
                    return Some(out);
                }
                self.finished = true;
                self.update_terminal_success();
                if !self.has_recoverable_partial_output() {
                    log::warn!(
                        "event=responses_from_chat_empty_terminal_completed model={} response_id={} saw_frame=true output_text_len={} pending_tool_calls={}",
                        self.model(),
                        self.response_id(),
                        self.output_text.len(),
                        self.pending_tool_calls.len()
                    );
                } else {
                    log::warn!(
                        "event=responses_from_chat_done_completed model={} response_id={} saw_frame=true output_text_len={} pending_tool_calls={}",
                        self.model(),
                        self.response_id(),
                        self.output_text.len(),
                        self.pending_tool_calls.len()
                    );
                }
                return Some(self.completed_event());
            }
            return None;
        }

        let value = Self::data_json(lines)?;
        self.remember_meta(&value);
        let mut out = Vec::new();
        if !self.created_sent {
            out.extend(self.handshake_events());
        }

        let delta = Self::choice_delta_text(&value);
        self.merge_tool_call_delta(&value);
        if !delta.is_empty() {
            if !self.message_item_opened {
                self.message_item_opened = true;
                let contract = super::super::ChatToResponsesLifecycle {
                    response_id: self.response_id(),
                    model: self.model(),
                    created_at: self.created(),
                    item_id: self.item_id(),
                    output_text: String::new(),
                    response: serde_json::json!({
                        "id": self.response_id(),
                        "object": "response",
                        "created_at": self.created(),
                        "created": self.created(),
                        "model": self.model(),
                        "status": "in_progress",
                    }),
                };
                out.extend(Self::sse_event(
                    "response.output_item.added",
                    contract.output_item_added_event(),
                ));
                out.extend(Self::sse_event(
                    "response.content_part.added",
                    contract.content_part_added_event(),
                ));
            }
            self.output_text.push_str(delta.as_str());
            out.extend(self.delta_event(delta.as_str()));
        }
        out.extend(self.emit_tool_call_events());
        let terminal_indices = Self::tool_call_terminal_indices(&value);
        if !terminal_indices.is_empty() {
            out.extend(self.emit_tool_call_done_events(&terminal_indices));
        }

        (!out.is_empty()).then_some(out)
    }

    fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        loop {
            match self
                .upstream
                .recv_timeout(stream_wait_timeout(self.last_upstream_activity))
            {
                Ok(UpstreamSseFramePumpItem::Frame(frame)) => {
                    self.last_upstream_activity = Instant::now();
                    self.saw_upstream_frame = true;
                    mark_first_response_ms(&self.usage_collector, self.request_started_at);
                    if let Some(chunk) = self.handle_frame(&frame) {
                        return Ok(chunk);
                    }
                    continue;
                }
                Ok(UpstreamSseFramePumpItem::Eof) => {
                    if !self.finished {
                        let pending_indices = self.pending_tool_call_indices();
                        if !pending_indices.is_empty() {
                            self.finished = true;
                            self.update_terminal_success();
                            let mut out = self.emit_tool_call_done_events(&pending_indices);
                            log::warn!(
                                "event=responses_from_chat_eof_flush_pending model={} response_id={} output_text_len={} pending_tool_calls={}",
                                self.model(),
                                self.response_id(),
                                self.output_text.len(),
                                self.pending_tool_calls.len()
                            );
                            out.extend(self.completed_event());
                            return Ok(out);
                        }
                        self.finished = true;
                        if self.saw_upstream_frame {
                            self.update_terminal_success();
                            if !self.has_recoverable_partial_output() {
                                log::warn!(
                                    "event=responses_from_chat_eof_empty_terminal_completed model={} response_id={} output_text_len={} pending_tool_calls={}",
                                    self.model(),
                                    self.response_id(),
                                    self.output_text.len(),
                                    self.pending_tool_calls.len()
                                );
                            } else {
                                log::warn!(
                                    "event=responses_from_chat_eof_completed model={} response_id={} output_text_len={} pending_tool_calls={}",
                                    self.model(),
                                    self.response_id(),
                                    self.output_text.len(),
                                    self.pending_tool_calls.len()
                                );
                            }
                            return Ok(self.completed_event());
                        }
                        if let Ok(mut collector) = self.usage_collector.lock() {
                            let hint = collector.upstream_error_hint.clone();
                            collector.terminal_error.get_or_insert_with(|| {
                                upstream_hint_or_stream_incomplete_message(hint.as_deref())
                            });
                        }
                    }
                    return Ok(Vec::new());
                }
                Ok(UpstreamSseFramePumpItem::Error(err)) => {
                    if let Some(result) =
                        self.synthesize_completion_from_partial_output("upstream_error")
                    {
                        return result;
                    }
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        collector
                            .terminal_error
                            .get_or_insert_with(|| classify_upstream_stream_read_error(&err));
                    }
                    self.finished = true;
                    return Ok(Vec::new());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !self.handshake_sent {
                        return Ok(self.handshake_events());
                    }
                    if stream_idle_timed_out(self.last_upstream_activity) {
                        if let Some(result) =
                            self.synthesize_completion_from_partial_output("idle_timeout")
                        {
                            return result;
                        }
                        if let Ok(mut collector) = self.usage_collector.lock() {
                            collector
                                .terminal_error
                                .get_or_insert_with(stream_idle_timeout_message);
                        }
                        self.finished = true;
                        return Ok(Vec::new());
                    }
                    if should_emit_keepalive(self.saw_upstream_frame) {
                        return Ok(SseKeepAliveFrame::OpenAIResponses.bytes().to_vec());
                    }
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    if let Some(result) =
                        self.synthesize_completion_from_partial_output("reader_disconnected")
                    {
                        return result;
                    }
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        let hint = collector.upstream_error_hint.clone();
                        collector.terminal_error.get_or_insert_with(|| {
                            hint.unwrap_or_else(stream_reader_disconnected_message)
                        });
                    }
                    self.finished = true;
                    return Ok(Vec::new());
                }
            }
        }
    }
}

impl Read for ResponsesFromChatCompletionsSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.out_cursor.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
            if self.finished {
                return Ok(0);
            }
            self.out_cursor = Cursor::new(self.next_chunk()?);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::http_bridge::stream_readers::{
        current_sse_keepalive_interval_ms, set_sse_keepalive_interval_ms,
    };
    use std::io::{Cursor, Read};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    struct FailAfterBytesReader {
        bytes: Vec<u8>,
        pos: usize,
        error_emitted: bool,
    }

    struct SlowFirstRead {
        bytes: Vec<u8>,
        pos: usize,
        delay: std::time::Duration,
        delayed: bool,
    }

    impl FailAfterBytesReader {
        fn new(bytes: &[u8]) -> Self {
            Self {
                bytes: bytes.to_vec(),
                pos: 0,
                error_emitted: false,
            }
        }
    }

    impl Read for FailAfterBytesReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.pos < self.bytes.len() {
                let remaining = self.bytes.len() - self.pos;
                let count = remaining.min(buf.len());
                buf[..count].copy_from_slice(&self.bytes[self.pos..self.pos + count]);
                self.pos += count;
                return Ok(count);
            }
            if !self.error_emitted {
                self.error_emitted = true;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "connection reset by peer",
                ));
            }
            Ok(0)
        }
    }

    impl SlowFirstRead {
        fn new(bytes: &[u8], delay_ms: u64) -> Self {
            Self {
                bytes: bytes.to_vec(),
                pos: 0,
                delay: std::time::Duration::from_millis(delay_ms),
                delayed: false,
            }
        }
    }

    impl Read for SlowFirstRead {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.delayed {
                self.delayed = true;
                std::thread::sleep(self.delay);
            }
            if self.pos >= self.bytes.len() {
                return Ok(0);
            }
            let remaining = self.bytes.len() - self.pos;
            let count = remaining.min(buf.len());
            buf[..count].copy_from_slice(&self.bytes[self.pos..self.pos + count]);
            self.pos += count;
            Ok(count)
        }
    }

    struct KeepaliveIntervalGuard {
        previous: u64,
    }

    impl KeepaliveIntervalGuard {
        fn set(interval_ms: u64) -> Self {
            let previous = current_sse_keepalive_interval_ms();
            set_sse_keepalive_interval_ms(interval_ms).expect("set sse keepalive interval");
            Self { previous }
        }
    }

    impl Drop for KeepaliveIntervalGuard {
        fn drop(&mut self) {
            let _ = set_sse_keepalive_interval_ms(self.previous);
        }
    }

    fn collector() -> Arc<Mutex<PassthroughSseCollector>> {
        Arc::new(Mutex::new(PassthroughSseCollector::default()))
    }

    #[test]
    fn chat_stream_does_not_stop_on_intermediate_finish_reason() {
        let payload = concat!(
            "event: chunk\n",
            "data: {\"id\":\"chatcmpl-mimo\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello \"},\"finish_reason\":\"stop\"}]}\n\n",
            "event: chunk\n",
            "data: {\"id\":\"chatcmpl-mimo\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"world\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            Cursor::new(payload.as_bytes().to_vec()),
            collector(),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(output.contains("\"delta\":\"hello \""));
        assert!(output.contains("\"delta\":\"world\""));
        assert!(output.contains("response.completed"));
    }

    #[test]
    fn chat_stream_emits_tool_call_after_leading_text_for_mimo_style_chunks() {
        let payload = concat!(
            "data: {\"id\":\"chatcmpl-mimo-tool-1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"我先检查一下。\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-mimo-tool-1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_mimo_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            Cursor::new(payload.as_bytes().to_vec()),
            collector(),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(output.contains("response.output_text.delta"));
        assert!(output.contains("我先检查一下"));
        assert!(output.contains("\"type\":\"response.output_item.added\""));
        assert!(output.contains("\"type\":\"response.function_call_arguments.delta\""));
        assert!(output.contains("\"type\":\"response.function_call_arguments.done\""));
        assert!(output.contains("\"type\":\"response.output_item.done\""));
        assert!(output.contains("\"type\":\"function_call\""));
        assert!(output.contains("\"name\":\"read_file\""));
        assert!(output.contains("\\\"path\\\":\\\"README.md\\\""));
    }

    #[test]
    fn chat_stream_emits_pure_tool_call_without_text() {
        let payload = concat!(
            "data: {\"id\":\"chatcmpl-mimo-tool-2\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_mimo_2\",\"type\":\"function\",\"function\":{\"name\":\"write_file\",\"arguments\":\"{\\\"file_path\\\":\\\"plans/site.md\\\",\\\"content\\\":\\\"plan\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            Cursor::new(payload.as_bytes().to_vec()),
            collector(),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(output.contains("\"type\":\"response.output_item.added\""));
        assert!(output.contains("\"type\":\"response.function_call_arguments.delta\""));
        assert!(output.contains("\"type\":\"response.function_call_arguments.done\""));
        assert!(output.contains("\"type\":\"response.output_item.done\""));
        assert!(output.contains("\"type\":\"function_call\""));
        assert!(output.contains("\"name\":\"write_file\""));
        assert!(output.contains("\\\"content\\\":\\\"plan\\\""));
        assert!(output.contains("response.completed"));
    }

    #[test]
    fn chat_stream_emits_split_tool_arguments_after_valid_json() {
        let payload = concat!(
            "data: {\"id\":\"chatcmpl-mimo-tool-3\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_mimo_3\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"REA\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-mimo-tool-3\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_mimo_3\",\"type\":\"function\",\"function\":{\"arguments\":\"DME.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            Cursor::new(payload.as_bytes().to_vec()),
            collector(),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(output.contains("\"type\":\"response.output_item.added\""));
        assert!(!output.contains("\"delta\":\"{\\\"path\\\":\\\"REA\""));
        assert!(output.contains("\"delta\":\"{\\\"path\\\":\\\"README.md\\\"}\""));
        assert!(output.contains("\"type\":\"response.function_call_arguments.done\""));
        assert!(output.contains("\\\"path\\\":\\\"README.md\\\""));
    }

    #[test]
    fn chat_stream_flushes_pending_tool_call_when_finish_reason_is_stop() {
        let payload = concat!(
            "data: {\"id\":\"chatcmpl-mimo-tool-4\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_mimo_4\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}]},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            Cursor::new(payload.as_bytes().to_vec()),
            collector(),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(output.contains("\"type\":\"response.output_item.added\""));
        assert!(output.contains("\"type\":\"response.function_call_arguments.done\""));
        assert!(output.contains("\"type\":\"response.output_item.done\""));
        assert!(output.contains("\"name\":\"read_file\""));
    }

    #[test]
    fn chat_stream_flushes_pending_tool_call_on_eof_without_done_frame() {
        let payload =
            "data: {\"id\":\"chatcmpl-mimo-tool-5\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_mimo_5\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}]},\"finish_reason\":null}]}\n\n";
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            Cursor::new(payload.as_bytes().to_vec()),
            collector(),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(output.contains("\"type\":\"response.function_call_arguments.done\""));
        assert!(output.contains("\"type\":\"response.output_item.done\""));
        assert!(output.contains("response.completed"));
    }

    #[test]
    fn chat_stream_completes_empty_success_stream() {
        let payload = concat!(
            "data: {\"id\":\"chatcmpl-glm-empty\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5.1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let collector = collector();
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            Cursor::new(payload.as_bytes().to_vec()),
            Arc::clone(&collector),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(output.contains("response.completed"));
        assert!(!output.contains("response.failed"));
        let collector = collector.lock().expect("collector");
        assert_eq!(
            collector.last_event_type.as_deref(),
            Some("response.completed")
        );
        assert!(collector.terminal_error.is_none());
        assert!(collector.saw_terminal);
    }

    #[test]
    fn responses_from_chat_emits_handshake_before_slow_first_upstream_frame() {
        let _keepalive_guard = KeepaliveIntervalGuard::set(5);
        let payload =
            "data: {\"id\":\"chatcmpl-slow-first-frame\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-5.5\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"late\"},\"finish_reason\":null}]}\n\n\
             data: [DONE]\n\n";
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            SlowFirstRead::new(payload.as_bytes(), 25),
            collector(),
            Instant::now(),
        );
        let mut first = vec![0_u8; 4096];
        let count = reader.read(&mut first).expect("read first handshake");
        let first = String::from_utf8(first[..count].to_vec()).expect("utf8");

        assert!(first.contains("response.created"));
        assert!(first.contains("response.in_progress"));
        assert!(!first.contains("response.output_text.delta"));

        let mut rest = String::new();
        reader
            .read_to_string(&mut rest)
            .expect("read delayed upstream frames");
        assert!(rest.contains("\"response_id\":\"resp_codexmanager_chat_adapter\""));
        assert!(rest.contains("\"delta\":\"late\""));
        assert!(rest.contains("response.completed"));
    }

    #[test]
    fn chat_stream_does_not_emit_invalid_tool_arguments_done_event() {
        let payload = concat!(
            "data: {\"id\":\"chatcmpl-glm-invalid-tool\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5.1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_glm_invalid_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"C:\\\\tmp\\\\a\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-glm-invalid-tool\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5.1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let collector = collector();
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            Cursor::new(payload.as_bytes().to_vec()),
            Arc::clone(&collector),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(!output.contains("\"name\":\"read_file\""));
        assert!(!output.contains("\"type\":\"response.function_call_arguments.delta\""));
        assert!(!output.contains("\"type\":\"response.function_call_arguments.done\""));
        assert!(!output.contains("\"item\":{\"type\":\"function_call\""));
        assert!(output.contains("response.completed"));
        let collector = collector.lock().expect("collector");
        assert_eq!(
            collector.last_event_type.as_deref(),
            Some("response.completed")
        );
        assert!(collector.terminal_error.is_none());
        assert!(collector.saw_terminal);
    }

    #[test]
    fn chat_stream_waits_until_tool_arguments_form_valid_json_before_emitting() {
        let payload = concat!(
            "data: {\"id\":\"chatcmpl-glm-tool-delayed\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5.1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_glm_delayed_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"abc\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-glm-tool-delayed\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5.1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_glm_delayed_1\",\"type\":\"function\",\"function\":{\"arguments\":\"def\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-glm-tool-delayed\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5.1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let collector = collector();
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            Cursor::new(payload.as_bytes().to_vec()),
            Arc::clone(&collector),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(output.contains("\"type\":\"response.output_item.added\""));
        assert!(output.contains("\"type\":\"response.function_call_arguments.delta\""));
        assert!(output.contains("\"type\":\"response.function_call_arguments.done\""));
        assert!(output.contains("\\\"path\\\":\\\"abcdef\\\""));
        let collector = collector.lock().expect("collector");
        assert_eq!(
            collector.last_event_type.as_deref(),
            Some("response.completed")
        );
        assert!(collector.terminal_error.is_none());
    }

    #[test]
    fn chat_stream_completes_on_disconnect_when_text_already_arrived() {
        let payload = concat!(
            "data: {\"id\":\"chatcmpl-mimo-disconnect-1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial answer\"},\"finish_reason\":null}]}\n\n"
        );
        let collector = collector();
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            FailAfterBytesReader::new(payload.as_bytes()),
            Arc::clone(&collector),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(output.contains("\"delta\":\"partial answer\""));
        assert!(output.contains("response.completed"));
        let collector = collector.lock().expect("collector");
        assert!(collector.saw_terminal);
        assert!(collector.terminal_error.is_none());
    }

    #[test]
    fn chat_stream_completes_on_disconnect_when_tool_call_already_arrived() {
        let payload = concat!(
            "data: {\"id\":\"chatcmpl-mimo-disconnect-2\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"mimo-v2.5-pro\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_disconnect_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}]},\"finish_reason\":null}]}\n\n"
        );
        let collector = collector();
        let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
            FailAfterBytesReader::new(payload.as_bytes()),
            Arc::clone(&collector),
            Instant::now(),
        );
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read stream");
        let output = String::from_utf8(buf).expect("utf8");
        assert!(output.contains("\"type\":\"response.function_call_arguments.done\""));
        assert!(output.contains("\"type\":\"response.output_item.done\""));
        assert!(output.contains("response.completed"));
        let collector = collector.lock().expect("collector");
        assert!(collector.saw_terminal);
        assert!(collector.terminal_error.is_none());
    }
}
