//! What a web server sends: an HTTP answer read whole and read as it streams (lengths, chunks, to
//! the end of the connection), an event stream (JMAP push), a download's `Content-Range`; what
//! the updater reads from GitHub: the release list, a `SHA256SUMS.txt`, the quota header and
//! release tags; and a language model's reply, in OpenAI's and Anthropic's formats, whole and
//! streamed as events (its text handed on as it comes, its tool calls built from their pieces),
//! whose errors never show the key.
#![no_main]

use std::io::{Read, Write};

use libfuzzer_sys::fuzz_target;
use mailcraft_model::AiApi;
use mailcraft_net::ai;
use mailcraft_net::http::{self, Request, Url};
use mailcraft_net::stream::Timeouts;
use mailcraft_net::updates;

/// The server's side of a connection: the input, a few bytes at a time; what's sent is dropped.
struct Server<'a> {
    data: &'a [u8],
    step: usize,
}

impl Read for Server<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.step.min(self.data.len()).min(buf.len());
        let (now, rest) = self.data.split_at(n);
        buf.get_mut(..n).unwrap_or_default().copy_from_slice(now);
        self.data = rest;
        Ok(n)
    }
}

impl Write for Server<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Timeouts for Server<'_> {
    fn set_read_timeout(&mut self, _: Option<std::time::Duration>) -> std::io::Result<()> {
        Ok(())
    }
}

/// A model's key, which a reply may quote back.
const KEY: &str = "sk-fuzz-key";

fuzz_target!(|data: &[u8]| {
    let Ok(url) = Url::parse("https://github.com/ZIDAN44/mailcraft/releases/download/v1/m.msi") else { return };
    let req = Request::get(&url.path, 4096);
    // The step the bytes come in at, from the first byte: chunk sizes and lines split anywhere.
    let step = usize::from(data.first().copied().unwrap_or(1)).max(1);
    let body = data.get(1..).unwrap_or_default();
    if let Ok(resp) = http::exchange(&mut Server { data: body, step }, &req, &url) {
        for api in [AiApi::OpenAi, AiApi::Anthropic] {
            if let Err(e) = ai::read_reply(api, &resp, Some(KEY)) {
                assert!(!e.to_string().contains(KEY), "an error showed the key: {e}");
            }
        }
    }
    // Streamed: what was handed on is what the answer says.
    let Ok(model) = Url::parse("https://models.example/v1/chat/completions") else { return };
    let ask = Request::post(&model.path, "application/json", b"{}".to_vec(), 1 << 16);
    for api in [AiApi::OpenAi, AiApi::Anthropic] {
        let mut said = String::new();
        let mut on = |text: &str, _: &str| said.push_str(text);
        match ai::read_stream(api, &mut Server { data: body, step }, &ask, &model, Some(KEY), &mut on) {
            Ok(a) => assert!(a.calls.len() <= 64, "more calls than an answer may make"),
            Err(e) => assert!(!e.to_string().contains(KEY), "an error showed the key: {e}"),
        }
    }
    let mut got = Vec::new();
    let _ = http::exchange_into(&mut Server { data: body, step }, &req, &url, |_| Ok(Some(&mut got)));
    assert!(got.len() <= 4096, "a body past the cap was written");
    // A JMAP push stream, chunked and not: events until the input ends or is refused.
    for chunked in [true, false] {
        let mut events = http::EventStream::new(Server { data: body, step }, chunked);
        while let Ok(Some(_)) = events.next(std::time::Duration::ZERO) {}
    }
    if let Ok(text) = std::str::from_utf8(body) {
        let _ = updates::content_range(text);
        let _ = updates::parse(text, Some("-windows-x64.msi"));
        let _ = updates::sum_for(text, "mailcraft-0.25.0-windows-x64.msi");
        let _ = updates::tag_version(text);
        let _ = updates::rate_limited(Some(text), 1_800_000_000);
        let _ = updates::Feed::local(text);
    }
});
