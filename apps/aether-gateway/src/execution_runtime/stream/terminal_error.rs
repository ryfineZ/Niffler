//! 正文留存可以关闭；终态错误识别必须始终运行，并独立于前置故障转移窗口。
use std::collections::BTreeMap;

use serde_json::Value;

use super::error::{inspect_prefetched_stream_body, StreamPrefetchInspection};

const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default)]
pub(super) struct SseTerminalErrorObserver {
    event: Vec<u8>,
    line_has_content: bool,
    oversized: bool,
    failure: Option<Value>,
}

impl SseTerminalErrorObserver {
    pub(super) fn push(&mut self, chunk: &[u8]) {
        if self.failure.is_some() {
            return;
        }
        for &byte in chunk {
            if !self.oversized {
                if self.event.len() < MAX_EVENT_BYTES {
                    self.event.push(byte);
                } else {
                    self.event.clear();
                    self.oversized = true;
                }
            }
            if byte == b'\n' {
                if !self.line_has_content {
                    self.inspect_event();
                    self.event.clear();
                    self.oversized = false;
                    if self.failure.is_some() {
                        return;
                    }
                }
                self.line_has_content = false;
            } else if byte != b'\r' {
                self.line_has_content = true;
            }
        }
    }

    fn inspect_event(&mut self) {
        if self.oversized || self.event.is_empty() {
            return;
        }
        if let StreamPrefetchInspection::EmbeddedError(error) =
            inspect_prefetched_stream_body(&BTreeMap::new(), &self.event)
        {
            self.failure = Some(error);
        }
    }

    pub(super) fn finish(&mut self) {
        // 某些上游在最后一条 JSON 后直接 EOF；仍只接受完整、可解析的事件。
        if self.failure.is_none() && !self.event.is_empty() && !self.oversized {
            self.event.extend_from_slice(b"\n\n");
            self.inspect_event();
        }
    }

    pub(super) fn failure(&self) -> Option<&Value> {
        self.failure.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAILED: &str = "event: response.failed\r\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"server_is_overloaded\",\"message\":\"Selected model is at capacity. Please try a different model.\"}}}\r\n\r\n";

    #[test]
    fn catches_failure_at_every_chunk_boundary_after_output() {
        for split in 0..=FAILED.len() {
            let mut observer = SseTerminalErrorObserver::default();
            observer
                .push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n");
            observer.push(&FAILED.as_bytes()[..split]);
            observer.push(&FAILED.as_bytes()[split..]);
            assert_eq!(
                observer.failure().unwrap()["error"]["code"],
                "server_is_overloaded"
            );
        }
    }

    #[test]
    fn ignores_error_words_in_normal_output_and_bounds_large_events() {
        let mut observer = SseTerminalErrorObserver::default();
        observer.push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"Selected model is at capacity.\"}\n\n");
        assert!(observer.failure().is_none());
        observer.push(&vec![b'x'; MAX_EVENT_BYTES + 10]);
        assert!(observer.event.len() <= MAX_EVENT_BYTES);
        observer.push(b"\n\n");
        observer.push(FAILED.as_bytes());
        assert!(observer.failure().is_some());
    }

    #[test]
    fn reads_error_event_without_trailing_blank_line() {
        let mut observer = SseTerminalErrorObserver::default();
        observer.push(b"event: error\ndata: {\"code\":\"slow_down\",\"message\":\"Selected model is at capacity.\"}");
        observer.finish();
        assert_eq!(observer.failure().unwrap()["error"]["code"], "slow_down");
    }
}
