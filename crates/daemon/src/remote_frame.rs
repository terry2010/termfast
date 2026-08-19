//! Remote terminal frame protocol — desktop ↔ mobile communication via relay.
//!
//! Frame format (before encryption, all multi-byte integers big-endian):
//!   [version:1] [type:1] [terminal_id:4] [payload_len:4] [payload:N]
//!
//! payload_len bit 31 = compressed flag, bits 0-30 = actual length (max 64KB)

use bytes::Bytes;
use std::io;

pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_OUTPUT_DATA: usize = 65536;
/// HISTORY payload = [seq:4][is_last:1][data], so max data = 65536 - 5 = 65531
pub const MAX_HISTORY_DATA: usize = 65531;

// Frame types
pub const HELLO: u8 = 0x00;
pub const LIST_REQUEST: u8 = 0x01;
pub const LIST_RESPONSE: u8 = 0x02;
pub const SUBSCRIBE: u8 = 0x03;
pub const UNSUBSCRIBE: u8 = 0x04;
pub const OUTPUT: u8 = 0x05;
pub const INPUT: u8 = 0x06;
pub const RESIZE: u8 = 0x07;
pub const GOODBYE: u8 = 0x08;
pub const ERROR: u8 = 0x09;
pub const HISTORY: u8 = 0x0A;
pub const REDRAW_REQUEST: u8 = 0x0B;
pub const NOTIFY: u8 = 0x0C;
pub const OK: u8 = 0x0D;
pub const INPUT_ANSWER: u8 = 0x0E;
pub const QUESTION_RESOLVED: u8 = 0x0F;
pub const FILE_REQUEST: u8 = 0x10;
pub const DESKTOP_PAIR: u8 = 0x11;
pub const INFO_REQUEST: u8 = 0x12;
pub const INFO_RESPONSE: u8 = 0x13;
pub const NEW_TERMINAL: u8 = 0x14;
pub const CLOSE_TERMINAL: u8 = 0x15;

/// In-memory frame representation.
#[derive(Debug, Clone)]
pub struct Frame {
    pub version: u8,
    pub frame_type: u8,
    pub terminal_id: u32,
    pub payload: Vec<u8>,
    pub compressed_flag: bool,
}

impl Frame {
    pub fn new(frame_type: u8, terminal_id: u32, payload: Vec<u8>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            frame_type,
            terminal_id,
            payload,
            compressed_flag: false,
        }
    }

    pub fn hello(capabilities: u16, random: &[u8; 32]) -> Self {
        let mut payload = Vec::with_capacity(34);
        payload.extend_from_slice(&capabilities.to_be_bytes());
        payload.extend_from_slice(random);
        Self::new(HELLO, 0, payload)
    }

    pub fn list_request() -> Self {
        Self::new(LIST_REQUEST, 0, Vec::new())
    }

    pub fn list_response(terminal_id: u32, json: &str) -> Self {
        Self::new(LIST_RESPONSE, terminal_id, json.as_bytes().to_vec())
    }

    pub fn subscribe(terminal_id: u32) -> Self {
        Self::new(SUBSCRIBE, terminal_id, Vec::new())
    }

    pub fn unsubscribe(terminal_id: u32) -> Self {
        Self::new(UNSUBSCRIBE, terminal_id, Vec::new())
    }

    pub fn output(terminal_id: u32, data: &[u8]) -> Self {
        Self::new(OUTPUT, terminal_id, data.to_vec())
    }

    pub fn input(terminal_id: u32, data: &[u8]) -> Self {
        Self::new(INPUT, terminal_id, data.to_vec())
    }

    pub fn resize(terminal_id: u32, cols: u16, rows: u16) -> Self {
        let mut payload = Vec::with_capacity(4);
        payload.extend_from_slice(&cols.to_be_bytes());
        payload.extend_from_slice(&rows.to_be_bytes());
        Self::new(RESIZE, terminal_id, payload)
    }

    pub fn goodbye() -> Self {
        Self::new(GOODBYE, 0, Vec::new())
    }

    pub fn error(msg: &str) -> Self {
        Self::new(ERROR, 0, msg.as_bytes().to_vec())
    }

    /// Construct an ERROR frame with a specific terminal_id.
    pub fn error_with_terminal(terminal_id: u32, msg: &str) -> Self {
        Self::new(ERROR, terminal_id, msg.as_bytes().to_vec())
    }

    pub fn history(terminal_id: u32, seq: u32, is_last: bool, data: &[u8]) -> Self {
        let mut payload = Vec::with_capacity(5 + data.len());
        payload.extend_from_slice(&seq.to_be_bytes());
        payload.push(if is_last { 1 } else { 0 });
        payload.extend_from_slice(data);
        Self::new(HISTORY, terminal_id, payload)
    }

    pub fn redraw_request(terminal_id: u32) -> Self {
        Self::new(REDRAW_REQUEST, terminal_id, Vec::new())
    }

    pub fn ok(terminal_id: u32) -> Self {
        Self::new(OK, terminal_id, Vec::new())
    }

    /// OK frame with JSON payload (used by NEW_TERMINAL response).
    pub fn ok_with_payload(terminal_id: u32, json: &str) -> Self {
        Self::new(OK, terminal_id, json.as_bytes().to_vec())
    }

    pub fn info_request() -> Self {
        Self::new(INFO_REQUEST, 0, Vec::new())
    }

    pub fn info_response(json: &str) -> Self {
        Self::new(INFO_RESPONSE, 0, json.as_bytes().to_vec())
    }

    /// NEW_TERMINAL: payload is JSON { "shell": "zsh", "name": "Terminal 1" }
    pub fn new_terminal(shell: Option<&str>, name: Option<&str>) -> Self {
        let json = serde_json::json!({
            "shell": shell,
            "name": name,
        });
        Self::new(NEW_TERMINAL, 0, json.to_string().as_bytes().to_vec())
    }

    pub fn close_terminal(terminal_id: u32) -> Self {
        Self::new(CLOSE_TERMINAL, terminal_id, Vec::new())
    }

    /// Construct a NOTIFY frame with JSON payload.
    /// payload = JSON {event_type, title, body, terminal_id, timestamp, ...}
    pub fn notify(terminal_id: u32, json: &str) -> Self {
        Self::new(NOTIFY, terminal_id, json.as_bytes().to_vec())
    }

    /// Construct an INPUT_ANSWER frame.
    /// payload = JSON {question_id, answer}
    pub fn input_answer(terminal_id: u32, question_id: &str, answer: &str) -> Self {
        let json = serde_json::json!({
            "question_id": question_id,
            "answer": answer,
        });
        Self::new(INPUT_ANSWER, terminal_id, json.to_string().into_bytes())
    }

    /// Construct a QUESTION_RESOLVED frame.
    /// payload = JSON {question_id, answer}
    pub fn question_resolved(terminal_id: u32, question_id: &str, answer: &str) -> Self {
        let json = serde_json::json!({
            "question_id": question_id,
            "answer": answer,
        });
        Self::new(QUESTION_RESOLVED, terminal_id, json.to_string().into_bytes())
    }

    /// Construct a FILE_REQUEST frame.
    /// payload = file_path (UTF-8 string)
    pub fn file_request(terminal_id: u32, file_path: &str) -> Self {
        Self::new(FILE_REQUEST, terminal_id, file_path.as_bytes().to_vec())
    }

    /// Construct a DESKTOP_PAIR frame.
    /// payload = JSON {action, pairing_id, pairing_key_hex, pairing_jwt, peer_name, pairing_type, role}
    pub fn desktop_pair(json: &str) -> Self {
        Self::new(DESKTOP_PAIR, 0, json.as_bytes().to_vec())
    }

    /// Construct a DESKTOP_PAIR response frame (pair_ok / pair_error).
    /// Uses the same DESKTOP_PAIR frame type so the sender can deserialize it
    /// with the same message struct (action field distinguishes request vs response).
    pub fn desktop_pair_response(json: &str) -> Self {
        Self::new(DESKTOP_PAIR, 0, json.as_bytes().to_vec())
    }

    /// Serialize frame to bytes (for encryption + transmission).
    pub fn serialize(&self) -> Vec<u8> {
        let payload_len = (self.payload.len() as u32) | (if self.compressed_flag { 0x80000000 } else { 0 });
        let mut buf = Vec::with_capacity(10 + self.payload.len());
        buf.push(self.version);
        buf.push(self.frame_type);
        buf.extend_from_slice(&self.terminal_id.to_be_bytes());
        buf.extend_from_slice(&payload_len.to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Deserialize frame from a reader (after decryption).
    pub fn deserialize(data: &[u8]) -> io::Result<Self> {
        if data.len() < 10 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too short"));
        }
        let version = data[0];
        let frame_type = data[1];
        let terminal_id = u32::from_be_bytes([data[2], data[3], data[4], data[5]]);
        let payload_len_raw = u32::from_be_bytes([data[6], data[7], data[8], data[9]]);
        let compressed_flag = (payload_len_raw & 0x80000000) != 0;
        let actual_len = (payload_len_raw & 0x7FFFFFFF) as usize;
        if data.len() < 10 + actual_len {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "incomplete payload"));
        }
        let payload = data[10..10 + actual_len].to_vec();
        Ok(Self {
            version,
            frame_type,
            terminal_id,
            payload,
            compressed_flag,
        })
    }

    /// Parse resize payload (cols, rows).
    pub fn parse_resize(&self) -> Option<(u16, u16)> {
        if self.payload.len() < 4 {
            return None;
        }
        let cols = u16::from_be_bytes([self.payload[0], self.payload[1]]);
        let rows = u16::from_be_bytes([self.payload[2], self.payload[3]]);
        Some((cols, rows))
    }

    /// Parse hello payload (capabilities, random).
    pub fn parse_hello(&self) -> Option<(u16, [u8; 32])> {
        if self.payload.len() < 34 {
            return None;
        }
        let caps = u16::from_be_bytes([self.payload[0], self.payload[1]]);
        let mut random = [0u8; 32];
        random.copy_from_slice(&self.payload[2..34]);
        Some((caps, random))
    }
}

/// Fixed-capacity ring buffer for terminal history.
pub struct RingBuffer {
    buf: std::collections::VecDeque<Bytes>,
    capacity_bytes: usize,
    current_bytes: usize,
}

impl RingBuffer {
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            buf: std::collections::VecDeque::new(),
            capacity_bytes,
            current_bytes: 0,
        }
    }

    pub fn push(&mut self, data: Bytes) {
        let data_len = data.len();
        self.current_bytes += data_len;
        self.buf.push_back(data);
        // Evict oldest while over capacity
        while self.current_bytes > self.capacity_bytes {
            if let Some(oldest) = self.buf.pop_front() {
                self.current_bytes -= oldest.len();
            } else {
                break;
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Bytes> {
        self.buf.iter()
    }

    pub fn total_bytes(&self) -> usize {
        self.current_bytes
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.current_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_serialize_deserialize() {
        let frame = Frame::output(42, b"hello world");
        let serialized = frame.serialize();
        let deserialized = Frame::deserialize(&serialized).unwrap();
        assert_eq!(deserialized.frame_type, OUTPUT);
        assert_eq!(deserialized.terminal_id, 42);
        assert_eq!(deserialized.payload, b"hello world");
    }

    #[test]
    fn test_frame_hello() {
        let random = [0xABu8; 32];
        let frame = Frame::hello(0x0001, &random);
        let (caps, rand) = frame.parse_hello().unwrap();
        assert_eq!(caps, 1);
        assert_eq!(rand, random);
    }

    #[test]
    fn test_frame_resize() {
        let frame = Frame::resize(5, 120, 40);
        let (cols, rows) = frame.parse_resize().unwrap();
        assert_eq!(cols, 120);
        assert_eq!(rows, 40);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let mut rb = RingBuffer::new(100);
        rb.push(Bytes::from(vec![0u8; 60]));
        assert_eq!(rb.total_bytes(), 60);
        rb.push(Bytes::from(vec![0u8; 60]));
        // 120 > 100, should evict first 60
        assert_eq!(rb.total_bytes(), 60);
        // Only the second chunk should remain
        let total: usize = rb.iter().map(|b| b.len()).sum();
        assert_eq!(total, 60);
    }

    #[test]
    fn test_ring_buffer_clear() {
        let mut rb = RingBuffer::new(1000);
        rb.push(Bytes::from(vec![1u8; 100]));
        rb.push(Bytes::from(vec![2u8; 100]));
        rb.clear();
        assert_eq!(rb.total_bytes(), 0);
        assert_eq!(rb.iter().count(), 0);
    }

    #[test]
    fn test_frame_compressed_flag() {
        let mut frame = Frame::output(1, b"compressed data");
        frame.compressed_flag = true;
        let serialized = frame.serialize();
        let deserialized = Frame::deserialize(&serialized).unwrap();
        assert!(deserialized.compressed_flag);
    }

    #[test]
    fn test_frame_too_short() {
        assert!(Frame::deserialize(b"short").is_err());
    }

    #[test]
    fn test_frame_history() {
        let frame = Frame::history(5, 0, true, b"snapshot");
        assert_eq!(frame.frame_type, HISTORY);
        assert_eq!(frame.terminal_id, 5);
        // payload = [seq:4][is_last:1][data]
        assert_eq!(frame.payload.len(), 5 + 8);
        let seq = u32::from_be_bytes([frame.payload[0], frame.payload[1], frame.payload[2], frame.payload[3]]);
        assert_eq!(seq, 0);
        assert_eq!(frame.payload[4], 1); // is_last
    }

    #[test]
    fn test_frame_desktop_pair() {
        let json = r#"{"action":"pair","pairing_id":"abc","role":"server"}"#;
        let frame = Frame::desktop_pair(json);
        assert_eq!(frame.frame_type, DESKTOP_PAIR);
        assert_eq!(frame.terminal_id, 0);
        assert_eq!(frame.payload, json.as_bytes());

        // Round-trip serialize → deserialize
        let serialized = frame.serialize();
        let deserialized = Frame::deserialize(&serialized).unwrap();
        assert_eq!(deserialized.frame_type, DESKTOP_PAIR);
        assert_eq!(deserialized.terminal_id, 0);
        assert_eq!(deserialized.payload, json.as_bytes());
    }

    #[test]
    fn test_frame_desktop_pair_response() {
        let json = r#"{"action":"pair_ok","pairing_id":"abc"}"#;
        let frame = Frame::desktop_pair_response(json);
        assert_eq!(frame.frame_type, DESKTOP_PAIR);
        assert_eq!(frame.terminal_id, 0);
        assert_eq!(frame.payload, json.as_bytes());

        // Round-trip
        let serialized = frame.serialize();
        let deserialized = Frame::deserialize(&serialized).unwrap();
        assert_eq!(deserialized.frame_type, DESKTOP_PAIR);
        assert_eq!(deserialized.payload, json.as_bytes());
    }
}
