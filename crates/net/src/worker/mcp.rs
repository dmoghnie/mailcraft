//! The MCP servers the person added, in the pool: a connection kept for each (a program left
//! running between calls), made when first wanted and again when it broke or the server's
//! settings changed; each piece of work on a thread of its own, one at a time for a server.
//! A server's key is read from the keychain on that thread (the engine never holds it).

use super::routing::spawn;
use super::*;
use crate::mcp::McpConn;
use mailcraft_model::{McpDone, McpTarget, McpWork};

#[cfg(test)]
mod tests;

/// At most this many pieces of work for MCP servers at once: past that, one is answered at once
/// with why, so a script run in a loop can't start threads without end.
const MCP_MOST: usize = 4;

/// A server's connection, and where it was made to.
#[derive(Default)]
pub(super) struct Slot {
    target: Option<McpTarget>,
    conn: Option<McpConn>,
}

/// One of the [`MCP_MOST`] places of the pool's `mcp_busy`; given back when dropped.
struct Busy(Arc<AtomicUsize>);

impl Busy {
    fn take(busy: &Arc<AtomicUsize>) -> Option<Busy> {
        let taken = busy.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < MCP_MOST).then_some(n + 1));
        taken.ok().map(|_| Busy(busy.clone()))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Pool {
    pub(super) fn mcp(&self, work: McpWork) {
        match work {
            McpWork::List { server, target, key } => {
                let (name, listed) = (server.clone(), server.clone());
                let failed = move |error: String| McpDone::Listed { server: name.clone(), tools: Vec::new(), error: Some(error) };
                self.mcp_on(server, target, key, failed, move |c| c.tools().map(|tools| McpDone::Listed { server: listed, tools, error: None }));
            }
            McpWork::Call { id, server, target, key, tool, args } => {
                let failed = move |error: String| McpDone::Called { id, result: None, error: Some(error) };
                self.mcp_on(server, target, key, failed, move |c| c.call(&tool, &args).map(|r| McpDone::Called { id, result: Some(r), error: None }));
            }
            // Its program ends as its connection goes, now or once the work on it ends.
            McpWork::Close { server } => {
                lock(&self.mcp).remove(&server);
            }
        }
    }

    /// Do `work` on server `server`'s connection, made first when there's none to `target`;
    /// what it did, or `failed` with why, sent on. A connection that failed is let go, to be
    /// made again next time.
    fn mcp_on(
        &self,
        server: String,
        target: McpTarget,
        key: Option<String>,
        failed: impl Fn(String) -> McpDone + Clone + Send + 'static,
        work: impl FnOnce(&mut McpConn) -> crate::Result<McpDone> + Send + 'static,
    ) {
        let Some(busy) = Busy::take(&self.mcp_busy) else {
            let why = format!("MailCraft is already waiting for {MCP_MOST} MCP servers; try again once one answers");
            return self.sink.emit(Event::Mcp { done: failed(why) });
        };
        let slot = lock(&self.mcp).entry(server).or_default().clone();
        let (sink, creds, fail) = (self.sink.clone(), self.creds.clone(), failed.clone());
        let spawned = spawn("mail-mcp", move || {
            let _busy = busy;
            let mut slot = lock(&slot);
            let done = slot.work(&target, key.as_deref(), creds.as_ref(), work).unwrap_or_else(|e| {
                slot.conn = None;
                fail(said(&e))
            });
            sink.emit(Event::Mcp { done });
        });
        if let Err(e) = spawned {
            self.sink.emit(Event::Mcp { done: failed(e.to_string()) });
        }
    }
}

/// Why a piece of work failed, for the person: what the client said as it is, without the
/// "unexpected server response" its kind of error puts before it (a program that couldn't be
/// started isn't a server's answer).
fn said(e: &crate::NetError) -> String {
    match e {
        crate::NetError::Protocol(why) => why.clone(),
        e => e.to_string(),
    }
}

impl Slot {
    /// `work` on the connection, made first when there's none to `target`.
    fn work(
        &mut self,
        target: &McpTarget,
        key: Option<&str>,
        creds: &dyn Credentials,
        work: impl FnOnce(&mut McpConn) -> crate::Result<McpDone>,
    ) -> crate::Result<McpDone> {
        if self.conn.is_none() || self.target.as_ref() != Some(target) {
            self.conn = None;
            let kept = key.map(|name| creds.get_key(name));
            if matches!(kept, Some(None)) {
                return Err(crate::NetError::Auth("no key is kept for this server".into()));
            }
            self.conn = Some(McpConn::connect(target, kept.flatten().as_deref())?);
            self.target = Some(target.clone());
        }
        match self.conn.as_mut() {
            Some(c) => work(c),
            None => Err(crate::NetError::Protocol("no connection to the server".into())),
        }
    }
}
