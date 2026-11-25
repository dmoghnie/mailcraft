//! What an MCP server on the web sends MailCraft's AI: JSON, one message or a batch, and
//! server-sent events, each message's tools (names checked, texts and schemas capped) and a
//! tool call's answer (its text parts cut, its structured content when it fits).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let tools = mailcraft_net::mcp::read_as_sent(&text);
    // A tool is at least `{"name":"x"}`, in the JSON and again in the events.
    assert!(tools <= data.len(), "more tools than the input could hold");
});
