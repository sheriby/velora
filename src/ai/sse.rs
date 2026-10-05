//! text/event-stream 解析。
//!
//! 只实现 OpenAI 兼容服务实际会用到的那部分 SSE 规范:事件之间用空行分隔,
//! `data:` 行携带载荷,`:` 开头是心跳注释,`event:`/`id:`/`retry:` 一律忽略。
//! 增量喂字节,跨 chunk 的半行留在缓冲区,凑齐再吐事件。

/// 一条已凑齐的 SSE 事件,载荷为拼接后的 `data` 字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SseEvent {
    pub(crate) data: String,
}

#[derive(Default)]
pub(crate) struct SseParser {
    buffer: String,
}

impl SseParser {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 喂进一段新收到的文本,返回其中已完整出现的事件(按出现顺序)。
    pub(crate) fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buffer.push_str(chunk);
        let mut events = Vec::new();
        // 事件以空行结尾;\r\n 与 \n 都认。
        while let Some(end) = find_event_end(&self.buffer) {
            let (raw, rest) = self.buffer.split_at(end);
            let event = parse_event(raw);
            self.buffer = rest.to_string();
            if let Some(event) = event {
                events.push(event);
            }
        }
        events
    }

    /// 流结束时收尾:规范要求事件以空行终止,但个别代理会在最后一个 `data:`
    /// 后直接关流;此时把残留的完整行当作最后一条事件吐出来,不丢尾巴。
    pub(crate) fn finish(&mut self) -> Vec<SseEvent> {
        let raw = std::mem::take(&mut self.buffer);
        if raw.trim().is_empty() {
            return Vec::new();
        }
        parse_event(&raw).into_iter().collect()
    }
}

/// 找第一个事件终止符(空行)的结束偏移;找不到返回 `None`。
fn find_event_end(buffer: &str) -> Option<usize> {
    let bytes = buffer.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                if index + 1 < bytes.len() && bytes[index + 1] == b'\n' {
                    return Some(index + 2);
                }
                if index + 2 < bytes.len() && bytes[index + 1] == b'\r' && bytes[index + 2] == b'\n'
                {
                    return Some(index + 3);
                }
            }
            b'\r' => {
                if index + 1 < bytes.len() && bytes[index + 1] == b'\r' {
                    return Some(index + 2);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// 解析一条原始事件文本(不含结尾空行);没有 `data` 行(纯注释/空事件)返回 `None`。
fn parse_event(raw: &str) -> Option<SseEvent> {
    let mut data: Vec<&str> = Vec::new();
    for line in raw.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            // 规范:字段名后的单个空格属于分隔符,要去掉;其余空格是载荷的一部分。
            data.push(value.strip_prefix(' ').unwrap_or(value));
        }
        // event:/id:/retry: 对 chat completions 没有意义,直接忽略。
    }
    if data.is_empty() {
        return None;
    }
    Some(SseEvent {
        data: data.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datas(events: &[SseEvent]) -> Vec<String> {
        events.iter().map(|event| event.data.clone()).collect()
    }

    #[test]
    fn single_event_parses() {
        let mut parser = SseParser::new();
        assert_eq!(
            datas(&parser.feed("data: {\"a\":1}\n\n")),
            vec![r#"{"a":1}"#]
        );
        assert!(parser.finish().is_empty());
    }

    #[test]
    fn event_split_across_chunks_is_held_until_complete() {
        let mut parser = SseParser::new();
        assert!(parser.feed("data: hel").is_empty());
        assert!(parser.feed("lo\n").is_empty());
        assert_eq!(datas(&parser.feed("\ndata: x\n\n")), vec!["hello", "x"]);
    }

    #[test]
    fn crlf_and_comment_lines_are_tolerated() {
        let mut parser = SseParser::new();
        let events = parser.feed(": keep-alive\r\ndata: one\r\n\r\ndata: two\n\n");
        assert_eq!(datas(&events), vec!["one", "two"]);
    }

    #[test]
    fn multiple_data_lines_join_with_newline() {
        let mut parser = SseParser::new();
        let events = parser.feed("data: a\ndata: b\n\n");
        assert_eq!(datas(&events), vec!["a\nb"]);
    }

    #[test]
    fn data_line_without_space_after_colon_keeps_payload_verbatim() {
        let mut parser = SseParser::new();
        assert_eq!(datas(&parser.feed("data:[DONE]\n\n")), vec!["[DONE]"]);
    }

    #[test]
    fn events_without_data_are_dropped() {
        let mut parser = SseParser::new();
        assert!(parser.feed(": ping\n\n").is_empty());
        assert!(parser.feed("\n\n").is_empty());
    }

    #[test]
    fn finish_flushes_trailing_event_without_blank_line() {
        let mut parser = SseParser::new();
        assert_eq!(datas(&parser.feed("data: 1\n\ndata: tail")), vec!["1"]);
        assert_eq!(datas(&parser.finish()), vec!["tail"]);
    }

    #[test]
    fn finish_without_pending_data_yields_nothing() {
        let mut parser = SseParser::new();
        parser.feed("data: 1\n\n");
        assert!(parser.finish().is_empty());
    }
}
