//! Raw MCP/JSON-RPC frame capture. No JSON parsing or reserialization occurs.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Stdin,
    Stdout,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawFrame {
    pub direction: Direction,
    pub bytes: Vec<u8>,
}

pub fn record_frames(stdin: &[u8], stdout: &[u8]) -> Vec<RawFrame> {
    let mut frames = Vec::new();
    for bytes in split_frames(stdin) {
        frames.push(RawFrame {
            direction: Direction::Stdin,
            bytes,
        });
    }
    for bytes in split_frames(stdout) {
        frames.push(RawFrame {
            direction: Direction::Stdout,
            bytes,
        });
    }
    frames
}

fn split_frames(bytes: &[u8]) -> Vec<Vec<u8>> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut frames = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            frames.push(bytes[start..=index].to_vec());
            start = index + 1;
        }
    }
    if start < bytes.len() {
        frames.push(bytes[start..].to_vec());
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_raw_frame_bytes_and_order() {
        let frames = record_frames(b"{\"id\":1}\npartial", b"{\"id\":1}\n");
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].direction, Direction::Stdin);
        assert_eq!(frames[1].bytes, b"partial");
        assert_eq!(frames[2].direction, Direction::Stdout);
    }
}
