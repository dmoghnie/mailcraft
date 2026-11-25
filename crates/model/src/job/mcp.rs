//! MCP servers the person adds, as the network layer reaches them (a program MailCraft starts,
//! or a server on the web), the tools they list, and the work asked of them: tools listed, a
//! tool called, a sign-in to a server on the web, a connection closed.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where an MCP server is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum McpTarget {
    /// A program MailCraft starts, with what it's given; its key, when it has one, goes in the
    /// environment variable `key_env`.
    #[serde(rename_all = "camelCase")]
    Program {
        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key_env: Option<String>,
    },
    /// A server on the web, by its address; its key goes as a bearer token, or, once signed in
    /// to it (`oauth`), a token its authorization server gave.
    Web {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        oauth: Option<McpOAuth>,
    },
}

/// How MailCraft signs in to a server on the web that asks for it (MCP's authorization): the
/// app registered with the server's authorization server, which gives tokens good for that
/// server alone. The tokens themselves are in the keychain, never here.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct McpOAuth {
    /// The authorization server's name, as its metadata gave it.
    pub issuer: String,
    pub token_url: String,
    pub client_id: String,
    /// The server's address the tokens are for (RFC 8707).
    pub resource: String,
    /// What was asked for; empty when the server named nothing.
    pub scope: String,
}

impl Default for McpTarget {
    fn default() -> Self {
        McpTarget::Web { url: String::new(), oauth: None }
    }
}

/// A tool an MCP server listed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// What the server says it does.
    pub about: String,
    /// What it takes: a JSON Schema object.
    pub schema: Value,
    /// The server says it changes nothing (`readOnlyHint`): called without asking the person.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
}

/// Work for the network layer's MCP servers. `key`: the name the server's key is kept under
/// in the keychain, none when it has none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "work", rename_all = "camelCase")]
pub enum McpWork {
    /// List server `server`'s tools; answered by [`McpDone::Listed`].
    List { server: String, target: McpTarget, key: Option<String> },
    /// Call `tool` of server `server` with `args`; answered by [`McpDone::Called`], the same `id`.
    Call { id: u64, server: String, target: McpTarget, key: Option<String>, tool: String, args: Value },
    /// Sign in to server `server` at `url` in the browser, its tokens kept under `key`;
    /// answered by [`McpDone::SignedIn`].
    SignIn { server: String, url: String, key: String },
    /// Close the connection to server `server` (turned off, changed or removed): its program ends.
    Close { server: String },
}

/// What the network layer's MCP servers did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "done", rename_all = "camelCase")]
pub enum McpDone {
    /// Server `server`'s tools, or why they couldn't be listed.
    Listed { server: String, tools: Vec<McpTool>, error: Option<String> },
    /// What call `id` gave, or why it failed.
    Called { id: u64, result: Option<Value>, error: Option<String> },
    /// Signed in to server `server`: how, from now on; or why not.
    SignedIn { server: String, oauth: Option<McpOAuth>, error: Option<String> },
}
