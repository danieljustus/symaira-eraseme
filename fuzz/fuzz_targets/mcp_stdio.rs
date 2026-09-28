#![no_main]
#![allow(dead_code)]

use libfuzzer_sys::fuzz_target;

mod mcp;

struct NoopHandler;

impl mcp::handler::ToolHandler for NoopHandler {}

fuzz_target!(|input: &[u8]| {
    // Keep this in-process target bounded while still reaching the Go 10,000
    // level nesting boundary on generated inputs.
    if input.len() > 65_536 {
        return;
    }
    let mut output = Vec::new();
    let _ = mcp::stream::serve_stream(input, &mut output, &NoopHandler);
});
