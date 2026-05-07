use super::{
    chat_image_payload, classify_upstream_stream_read_error, collect_image_generation_data_urls,
    collect_response_output_text, mark_first_response_ms, merge_usage, should_emit_keepalive,
    stream_idle_timed_out, stream_idle_timeout_message, stream_reader_disconnected_message,
    stream_wait_timeout, upstream_hint_or_stream_incomplete_message, Arc, Cursor, Mutex,
    PassthroughSseCollector, Read, SseKeepAliveFrame, UpstreamSseFramePump,
    UpstreamSseFramePumpItem,
};
use serde_json::Value;
use std::collections::HashSet;
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
    id: Option<String>,
    model: Option<String>,
    created: Option<i64>,
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

    fn handle_frame(&mut self, lines: &[String]) -> Option<Vec<u8>> {
        let value = Self::data_json(lines)?;
        self.remember_meta(&value);
        let event_type = Self::event_type(lines, &value);
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
            self.update_terminal_success(event_type.as_deref());
            self.finished = true;
            out.extend(self.final_chunk());
            return Some(out);
        }
        if event_type.as_deref() == Some("response.output_item.done") {
            if let Some(images) = self.image_delta_chunk(&value) {
                return Some(images);
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
    response_id: Option<String>,
    model: Option<String>,
    created: Option<i64>,
    output_text: String,
    sequence_number: i64,
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
            response_id: None,
            model: None,
            created: None,
            output_text: String::new(),
            sequence_number: 0,
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
        let response = serde_json::json!({
            "id": self.response_id(),
            "object": "response",
            "created_at": self.created(),
            "created": self.created(),
            "model": self.model(),
            "status": "completed",
            "output": [{
                "id": self.item_id(),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": self.output_text,
                    "annotations": []
                }]
            }],
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
        out.extend(Self::sse_event(
            "response.completed",
            contract.completed_event(),
        ));
        out
    }

    fn update_terminal_success(&self) {
        if let Ok(mut collector) = self.usage_collector.lock() {
            collector.last_event_type = Some("response.completed".to_string());
            collector.saw_terminal = true;
        }
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

    fn has_finish_reason(value: &Value) -> bool {
        value
            .get("choices")
            .and_then(Value::as_array)
            .is_some_and(|choices| {
                choices.iter().any(|choice| {
                    choice
                        .get("finish_reason")
                        .is_some_and(|finish_reason| !finish_reason.is_null())
                })
            })
    }

    fn handle_frame(&mut self, lines: &[String]) -> Option<Vec<u8>> {
        if Self::data_text(lines)
            .as_deref()
            .is_some_and(|data| data.trim() == "[DONE]")
        {
            if !self.finished {
                self.finished = true;
                self.update_terminal_success();
                return Some(self.completed_event());
            }
            return None;
        }

        let value = Self::data_json(lines)?;
        self.remember_meta(&value);
        let mut out = Vec::new();
        if !self.created_sent {
            self.created_sent = true;
            let contract = super::super::ChatToResponsesLifecycle {
                response_id: self.response_id(),
                model: self.model(),
                created_at: self.created(),
                item_id: self.item_id(),
                output_text: self.output_text.clone(),
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
                "response.created",
                contract.created_event(),
            ));
            out.extend(Self::sse_event(
                "response.in_progress",
                contract.in_progress_event(),
            ));
            out.extend(Self::sse_event(
                "response.output_item.added",
                contract.output_item_added_event(),
            ));
            out.extend(Self::sse_event(
                "response.content_part.added",
                contract.content_part_added_event(),
            ));
        }

        let delta = Self::choice_delta_text(&value);
        if !delta.is_empty() {
            self.output_text.push_str(delta.as_str());
            out.extend(self.delta_event(delta.as_str()));
        }

        if Self::has_finish_reason(&value) {
            self.finished = true;
            self.update_terminal_success();
            out.extend(self.completed_event());
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
                        self.finished = true;
                        if self.saw_upstream_frame {
                            self.update_terminal_success();
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
