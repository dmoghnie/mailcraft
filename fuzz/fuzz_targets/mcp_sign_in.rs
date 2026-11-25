//! What an MCP server on the web and its authorization server say of signing in: a 401's
//! `WWW-Authenticate` challenge, the server's metadata (RFC 9728) and the authorization
//! server's (RFC 8414), each checked before MailCraft goes to any address they name.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = mailcraft_net::mcp::read_sign_in(&text);
});
