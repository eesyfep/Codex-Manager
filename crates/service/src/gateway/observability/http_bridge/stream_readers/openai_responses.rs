use super::{
    classify_upstream_stream_read_error, mark_first_response_ms, merge_usage,
    stream_idle_timed_out, stream_idle_timeout_message, stream_reader_disconnected_message,
    stream_wait_timeout, upstream_hint_or_stream_incomplete_message, Arc, Cursor, Mutex,
    OpenAIResponsesEvent, PassthroughSseCollector, Read, SseKeepAliveFrame, SseTerminal,
    UpstreamSseFramePump, UpstreamSseFramePumpItem,
};
use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem, GatewayStreamResponse};
use bytes::Bytes;
use serde_json::json;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

pub(crate) struct OpenAIResponsesPassthroughSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    request_started_at: Instant,
    last_upstream_activity: Instant,
    saw_upstream_frame: bool,
    handshake_sent: bool,
    synthetic_response_id: String,
    no_upstream_after_handshake_timeout: Option<Duration>,
    finished: bool,
}

struct GatewayByteStreamReader {
    stream: GatewayByteStream,
    current: Cursor<Bytes>,
    eof: bool,
}

impl GatewayByteStreamReader {
    fn new(stream: GatewayByteStream) -> Self {
        Self {
            stream,
            current: Cursor::new(Bytes::new()),
            eof: false,
        }
    }
}

impl Read for GatewayByteStreamReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.current.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
            if self.eof {
                return Ok(0);
            }
            match self.stream.recv() {
                Ok(GatewayByteStreamItem::Chunk(bytes)) => {
                    self.current = Cursor::new(bytes);
                }
                Ok(GatewayByteStreamItem::Eof) | Err(_) => {
                    self.eof = true;
                    return Ok(0);
                }
                Ok(GatewayByteStreamItem::Error(err)) => {
                    return Err(std::io::Error::other(err));
                }
            }
        }
    }
}

impl OpenAIResponsesPassthroughSseReader {
    pub(crate) fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        _keepalive_frame: SseKeepAliveFrame,
        request_started_at: Instant,
    ) -> Self {
        Self::from_stream_response(
            GatewayStreamResponse::from_blocking_response(upstream),
            usage_collector,
            SseKeepAliveFrame::OpenAIResponses,
            request_started_at,
        )
    }

    pub(crate) fn from_stream_response(
        upstream: GatewayStreamResponse,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        _keepalive_frame: SseKeepAliveFrame,
        request_started_at: Instant,
    ) -> Self {
        let upstream_reader = GatewayByteStreamReader::new(upstream.into_body());
        Self {
            upstream: UpstreamSseFramePump::from_reader(upstream_reader),
            out_cursor: Cursor::new(Vec::new()),
            usage_collector,
            request_started_at,
            last_upstream_activity: Instant::now(),
            saw_upstream_frame: false,
            handshake_sent: false,
            synthetic_response_id: "resp_codexmanager_openai_passthrough".to_string(),
            no_upstream_after_handshake_timeout: None,
            finished: false,
        }
    }

    pub(crate) fn with_no_upstream_after_handshake_timeout(
        mut self,
        timeout: Option<Duration>,
    ) -> Self {
        self.no_upstream_after_handshake_timeout = timeout;
        self
    }

    fn update_usage_from_event(&self, event: OpenAIResponsesEvent) {
        if let Ok(mut collector) = self.usage_collector.lock() {
            if let Some(event_type) = event.event_type {
                collector.last_event_type = Some(event_type);
            }
            merge_usage(&mut collector.usage, event.usage);
            if let Some(upstream_error_hint) = event.upstream_error_hint {
                collector.upstream_error_hint = Some(upstream_error_hint);
            }
            if let Some(terminal) = event.terminal {
                collector.saw_terminal = true;
                if let SseTerminal::Err(message) = terminal {
                    collector.terminal_error = Some(message);
                }
            }
        }
    }

    fn sse_event(event: &str, payload: serde_json::Value) -> Vec<u8> {
        format!(
            "event: {event}\ndata: {}\n\n",
            serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
        )
        .into_bytes()
    }

    fn handshake_events(&mut self) -> Vec<u8> {
        if self.handshake_sent {
            return Vec::new();
        }
        self.handshake_sent = true;
        if let Ok(mut collector) = self.usage_collector.lock() {
            collector.last_event_type = Some("response.in_progress".to_string());
        }
        let created = Self::sse_event(
            "response.created",
            json!({
                "type": "response.created",
                "response": {
                    "id": self.synthetic_response_id,
                    "object": "response",
                    "created_at": 0,
                    "model": "gpt-5.5",
                    "status": "in_progress"
                }
            }),
        );
        let in_progress = Self::sse_event(
            "response.in_progress",
            json!({
                "type": "response.in_progress",
                "response": {
                    "id": self.synthetic_response_id,
                    "object": "response",
                    "created_at": 0,
                    "model": "gpt-5.5",
                    "status": "in_progress"
                }
            }),
        );
        [created, in_progress].concat()
    }

    fn failed_event(&self, message: &str) -> Vec<u8> {
        Self::sse_event(
            "response.failed",
            json!({
                "type": "response.failed",
                "response": {
                    "id": self.synthetic_response_id,
                    "object": "response",
                    "created_at": 0,
                    "model": "gpt-5.5",
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

    fn finish_with_failed_event(&mut self, message: String) -> Vec<u8> {
        self.finished = true;
        let mut out = Vec::new();
        if !self.handshake_sent {
            out.extend(self.handshake_events());
        }
        if let Ok(mut collector) = self.usage_collector.lock() {
            collector.last_event_type = Some("response.failed".to_string());
            collector.saw_terminal = true;
            collector.upstream_error_hint.get_or_insert(message.clone());
            collector.terminal_error.get_or_insert(message.clone());
        }
        out.extend(self.failed_event(message.as_str()));
        out
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
                    if let Some(event) = OpenAIResponsesEvent::parse(&frame) {
                        let is_terminal = event.terminal.is_some();
                        self.update_usage_from_event(event);
                        if is_terminal {
                            self.finished = true;
                        }
                    }
                    return Ok(frame.concat().into_bytes());
                }
                Ok(UpstreamSseFramePumpItem::Eof) => {
                    let terminal_seen = self
                        .usage_collector
                        .lock()
                        .map(|collector| collector.saw_terminal)
                        .unwrap_or(false);
                    if !terminal_seen {
                        let hint = self
                            .usage_collector
                            .lock()
                            .ok()
                            .and_then(|collector| collector.upstream_error_hint.clone());
                        return Ok(self.finish_with_failed_event(
                            upstream_hint_or_stream_incomplete_message(hint.as_deref()),
                        ));
                    }
                    self.finished = true;
                    return Ok(Vec::new());
                }
                Ok(UpstreamSseFramePumpItem::Error(err)) => {
                    self.last_upstream_activity = Instant::now();
                    return Ok(
                        self.finish_with_failed_event(classify_upstream_stream_read_error(&err))
                    );
                }
                Err(RecvTimeoutError::Timeout) => {
                    if !self.saw_upstream_frame && !self.handshake_sent {
                        mark_first_response_ms(&self.usage_collector, self.request_started_at);
                        return Ok(self.handshake_events());
                    }
                    if self.handshake_sent
                        && !self.saw_upstream_frame
                        && self
                            .no_upstream_after_handshake_timeout
                            .is_some_and(|timeout| self.last_upstream_activity.elapsed() >= timeout)
                    {
                        return Ok(self.finish_with_failed_event(
                            "低质量中转兼容模式：上游首帧等待超时".to_string(),
                        ));
                    }
                    if stream_idle_timed_out(self.last_upstream_activity) {
                        return Ok(self.finish_with_failed_event(stream_idle_timeout_message()));
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let hint = self
                        .usage_collector
                        .lock()
                        .ok()
                        .and_then(|collector| collector.upstream_error_hint.clone());
                    return Ok(self.finish_with_failed_event(
                        hint.unwrap_or_else(stream_reader_disconnected_message),
                    ));
                }
            }
        }
    }
}

impl Read for OpenAIResponsesPassthroughSseReader {
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
