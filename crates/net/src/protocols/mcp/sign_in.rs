//! Signing in to an MCP server on the web that asks for it (MCP's authorization, 2025-11-25,
//! with the issuer check of 2026-07-28). The server's 401 names where its metadata is (RFC
//! 9728). That names its authorization server, whose own metadata (RFC 8414, or OpenID Connect
//! discovery) gives its endpoints. MailCraft registers there for the redirect to this computer
//! (RFC 7591) and signs in in the browser with PKCE, asking for tokens good for this server alone
//! (`resource`, RFC 8707). What the server and its authorization server send is hostile: sizes
//! are capped, and every address they name is checked as the server's own is (https, unless on
//! this computer), and is never on this computer unless the server is ([`reach`]).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{STARTING, VERSIONS, address};
use crate::http::{self, Request, Url};
use crate::oauth::{self, Provider};
use crate::{NetError, Result};
use mailcraft_model::McpOAuth;

#[cfg(test)]
mod tests;

/// The most read of a metadata document, or of a registration's answer.
const DOC_MOST: usize = 256 * 1024;
/// The longest `WWW-Authenticate` read.
const HEADER_MOST: usize = 8 * 1024;
/// The longest address, name, client id or scope kept.
const FIELD_MOST: usize = 2048;
/// The most of a challenge's parameters read.
const PARAMS_MOST: usize = 64;

/// What a server's 401 asks for: its `WWW-Authenticate`'s Bearer challenge (RFC 6750, 9728).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Challenge {
    /// Where its metadata is.
    pub metadata: Option<String>,
    /// What to ask for.
    pub scope: Option<String>,
}

/// What the keychain keeps for a server signed in to: its tokens, and the secret its
/// registration gave, if it gave one. Its `Debug` says which are there, never what they are.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Kept {
    pub access: String,
    /// When the access token runs out, in Unix seconds (a minute early).
    pub expires: i64,
    pub refresh: Option<String>,
    pub secret: Option<String>,
}

impl std::fmt::Debug for Kept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kept")
            .field("access", &"…")
            .field("expires", &self.expires)
            .field("refresh", &self.refresh.as_ref().map(|_| "…"))
            .field("secret", &self.secret.as_ref().map(|_| "…"))
            .finish()
    }
}

impl Kept {
    /// What the keychain holds, read; none for anything else (a key typed in by hand).
    pub fn read(text: &str) -> Option<Kept> {
        serde_json::from_str::<Kept>(text).ok().filter(|k| !k.access.is_empty())
    }

    /// As the keychain keeps it.
    pub fn text(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// The Bearer challenge in `header`: its `resource_metadata` and `scope`.
pub fn challenge(header: &str) -> Challenge {
    let mut found = Challenge::default();
    let mut bearer = false;
    for (name, value) in auth_params(short(header, HEADER_MOST).as_str()) {
        match (name.as_str(), value) {
            (scheme, None) => bearer = scheme.eq_ignore_ascii_case("bearer"),
            ("resource_metadata", Some(v)) if bearer => found.metadata = Some(v),
            ("scope", Some(v)) if bearer => found.scope = Some(v),
            _ => {}
        }
    }
    found
}

/// A challenge's words: each scheme alone, and each parameter's name (in lower case) with its
/// value, a quoted one unquoted.
fn auth_params(header: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut chars = header.chars().peekable();
    let blank = |c: &char| *c == ' ' || *c == '\t';
    while out.len() < PARAMS_MOST {
        while chars.peek().is_some_and(|c| *c == ',' || c.is_whitespace()) {
            chars.next();
        }
        let mut name = String::new();
        while let Some(c) = chars.next_if(|c| !c.is_whitespace() && *c != ',' && *c != '=') {
            name.push(c);
        }
        if name.is_empty() {
            // A stray `=`, or the end.
            if chars.next().is_none() {
                break;
            }
            continue;
        }
        while chars.next_if(blank).is_some() {}
        if chars.next_if_eq(&'=').is_none() {
            out.push((name, None));
            continue;
        }
        while chars.next_if(blank).is_some() {}
        let mut value = String::new();
        if chars.next_if_eq(&'"').is_some() {
            while let Some(c) = chars.next() {
                match c {
                    '\\' => value.extend(chars.next()),
                    '"' => break,
                    c => value.push(c),
                }
            }
        } else {
            while let Some(c) = chars.next_if(|c| *c != ',' && !c.is_whitespace()) {
                value.push(c);
            }
        }
        out.push((name.to_ascii_lowercase(), Some(value)));
    }
    out
}

/// A server's address as the resource its tokens are for (RFC 8707, as MCP forms it): scheme
/// and host in lower case, the port only when it isn't the usual one, no query or fragment, no
/// slash at the end.
pub fn resource_of(url: &str) -> Result<String> {
    let u = Url::parse(&address(url)?)?;
    let path = u.path.split(['?', '#']).next().unwrap_or_default().trim_end_matches('/');
    let host = u.host.to_ascii_lowercase();
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host };
    let usual = if u.https { 443 } else { 80 };
    let port = if u.port == usual { String::new() } else { format!(":{}", u.port) };
    Ok(format!("{}://{host}{port}{path}", if u.https { "https" } else { "http" }))
}

/// `scheme://host[:port]`, and the path after it ("", or from its `/`).
fn split_origin(url: &str) -> (&str, &str) {
    let after = url.find("://").map_or(0, |i| i.saturating_add(3));
    match url.get(after..).and_then(|rest| rest.find('/')) {
        Some(i) => url.split_at(after.saturating_add(i)),
        None => (url, ""),
    }
}

/// Where a resource's metadata may be (RFC 9728), in the order MCP tries them: under its
/// path, then at its host's root.
fn metadata_urls(resource: &str) -> Vec<String> {
    let (origin, path) = split_origin(resource);
    let mut urls = Vec::new();
    if !path.is_empty() {
        urls.push(format!("{origin}/.well-known/oauth-protected-resource{path}"));
    }
    urls.push(format!("{origin}/.well-known/oauth-protected-resource"));
    urls
}

/// Where an authorization server's metadata may be, in the order MCP tries them: RFC 8414's
/// place, then OpenID Connect's, each with the issuer's path.
fn server_urls(issuer: &str) -> Vec<String> {
    let (origin, path) = split_origin(issuer.trim_end_matches('/'));
    if path.is_empty() {
        return vec![format!("{origin}/.well-known/oauth-authorization-server"), format!("{origin}/.well-known/openid-configuration")];
    }
    vec![
        format!("{origin}/.well-known/oauth-authorization-server{path}"),
        format!("{origin}/.well-known/openid-configuration{path}"),
        format!("{origin}{path}/.well-known/openid-configuration"),
    ]
}

/// `url`, an address a server's sign-in names, if MailCraft goes there: as the server's own
/// address is checked, and never on this computer unless the server is (`local`): a server
/// elsewhere would have MailCraft send requests to whatever listens here.
fn reach(url: &str, local: bool) -> Result<String> {
    let checked = address(url)?;
    if !local && Url::parse(&checked).is_ok_and(|u| http::is_loopback(&u.host)) {
        return Err(NetError::Protocol("a server elsewhere named an address on this computer for its sign-in".into()));
    }
    Ok(checked)
}

/// The JSON object at `url` (reached as [`reach`] says); none when there's none there.
fn fetch(url: &str, local: bool) -> Result<Option<Map<String, Value>>> {
    let r = http::send(&Request::get(&reach(url, local)?, DOC_MOST).header("Accept", "application/json"))?;
    if !r.ok() {
        return Ok(None);
    }
    Ok(match serde_json::from_slice::<Value>(&r.body) {
        Ok(Value::Object(m)) => Some(m),
        _ => None,
    })
}

/// A string field of `doc`, kept to [`FIELD_MOST`] bytes; none when it's missing, empty or longer.
fn field<'a>(doc: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    doc.get(name).and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= FIELD_MOST)
}

/// A list of strings in `doc`, the others left out.
fn strings(doc: &Map<String, Value>, name: &str) -> Vec<String> {
    let list = doc.get(name).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    list.iter().filter_map(Value::as_str).filter(|s| s.len() <= FIELD_MOST).take(PARAMS_MOST).map(str::to_string).collect()
}

/// What a resource's metadata says, once it's checked to be the server's own (its resource,
/// or one the server's address is under): the resource tokens are for, the authorization
/// server's name, and the scopes it supports.
fn read_resource(doc: &Map<String, Value>, ours: &str) -> Result<(String, String, Vec<String>)> {
    let named = field(doc, "resource").map(resource_of).transpose()?;
    let covers = |theirs: &str| ours == theirs || ours.strip_prefix(theirs).is_some_and(|rest| rest.starts_with('/'));
    let Some(resource) = named.filter(|r| covers(r)) else {
        return Err(NetError::Protocol("the server's sign-in details are for another address".into()));
    };
    let issuer = doc.get("authorization_servers").and_then(Value::as_array).and_then(|a| a.iter().filter_map(Value::as_str).next());
    let issuer =
        issuer.filter(|s| !s.is_empty() && s.len() <= FIELD_MOST).ok_or_else(|| NetError::Protocol("the server names no sign-in server".into()))?;
    Ok((resource, issuer.to_string(), strings(doc, "scopes_supported")))
}

/// An authorization server, from its metadata.
#[derive(Debug, PartialEq, Eq)]
struct Server {
    auth_url: String,
    token_url: String,
    registration: Option<String>,
    iss_always: bool,
}

/// What authorization server `issuer`'s metadata says, once checked: it names itself `issuer`
/// (RFC 8414), takes PKCE with S256 (MCP: else MailCraft goes no further), and its endpoints
/// are addresses MailCraft reaches (`local`: as [`reach`] says).
fn read_server(doc: &Map<String, Value>, issuer: &str, local: bool) -> Result<Server> {
    if field(doc, "issuer") != Some(issuer) {
        return Err(NetError::Protocol("the sign-in server's details name another server".into()));
    }
    let endpoint = |name: &str| field(doc, name).map(|url| reach(url, local)).transpose();
    let missing = |what: &str| NetError::Protocol(format!("the sign-in server names no {what}"));
    let auth_url = endpoint("authorization_endpoint")?.ok_or_else(|| missing("sign-in page"))?;
    let token_url = endpoint("token_endpoint")?.ok_or_else(|| missing("token address"))?;
    if !strings(doc, "code_challenge_methods_supported").iter().any(|m| m == "S256") {
        return Err(NetError::Protocol("the sign-in server doesn't say it takes PKCE (S256), so MailCraft won't sign in there".into()));
    }
    let iss_always = doc.get("authorization_response_iss_parameter_supported").and_then(Value::as_bool).unwrap_or(false);
    Ok(Server { auth_url, token_url, registration: endpoint("registration_endpoint")?, iss_always })
}

/// The first JSON object at `urls` (reached as [`reach`] says), read by `read`; or why none
/// could be.
fn first<T>(urls: &[String], local: bool, none: &str, read: impl Fn(&Map<String, Value>) -> Result<T>) -> Result<T> {
    for url in urls {
        if let Some(doc) = fetch(url, local)? {
            return read(&doc);
        }
    }
    Err(NetError::Protocol(none.into()))
}

/// Server `url`'s authorization server, as a provider still to be registered with (no client
/// id yet), and where to register.
fn discover(url: &str, asked: &Challenge) -> Result<(Provider, Option<String>)> {
    let ours = resource_of(url)?;
    let local = Url::parse(&ours).is_ok_and(|u| http::is_loopback(&u.host));
    let urls = match &asked.metadata {
        Some(m) => vec![m.clone()],
        None => metadata_urls(&ours),
    };
    let (resource, issuer, scopes) = first(&urls, local, "the server gives no sign-in details", |d| read_resource(d, &ours))?;
    let server = first(&server_urls(&issuer), local, "the sign-in server gives no details", |d| read_server(d, &issuer, local))?;
    let scope = asked.scope.clone().unwrap_or_else(|| scopes.join(" "));
    let name = Url::parse(url).map(|u| u.host).unwrap_or_default();
    let p = Provider {
        id: "mcp".into(),
        name,
        auth_url: server.auth_url,
        token_url: server.token_url,
        scope: short(&scope, FIELD_MOST),
        client_id: String::new(),
        client_secret: None,
        resource: Some(resource),
        issuer: Some(issuer),
        iss_always: server.iss_always,
    };
    Ok((p, server.registration))
}

/// Register MailCraft at `endpoint` (RFC 7591), as an app on this computer with no secret of
/// its own, for `redirect`: its client id, and a secret if the server gave one anyway.
fn register(endpoint: &str, redirect: &str) -> Result<(String, Option<String>)> {
    let asked = json!({
        "client_name": "MailCraft",
        "redirect_uris": [redirect],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    let req = Request::post(endpoint, "application/json", asked.to_string().into_bytes(), DOC_MOST).header("Accept", "application/json");
    let r = http::send(&req)?;
    let answer = match serde_json::from_slice::<Value>(&r.body) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    if !r.ok() {
        let why = field(&answer, "error_description").or_else(|| field(&answer, "error")).unwrap_or("refused");
        return Err(NetError::Auth(format!("MailCraft couldn't register with the sign-in server: {}", short(why, 200))));
    }
    let id = field(&answer, "client_id").ok_or_else(|| NetError::Protocol("the sign-in server gave no client id".into()))?;
    Ok((id.to_string(), field(&answer, "client_secret").map(str::to_string)))
}

/// Ask server `url` with no token: its challenge, when it wants a sign-in.
fn probe(url: &str) -> Result<Challenge> {
    let hello = json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {"protocolVersion": VERSIONS[0], "capabilities": {}, "clientInfo": {"name": "MailCraft", "version": env!("CARGO_PKG_VERSION")}},
    });
    let req = Request::post(&address(url)?, "application/json", hello.to_string().into_bytes(), DOC_MOST)
        .header("Accept", "application/json, text/event-stream");
    let r = http::send_waiting(&req, STARTING)?;
    match r.status {
        401 => Ok(challenge(r.header("WWW-Authenticate").unwrap_or_default())),
        s if (200..300).contains(&s) => Err(NetError::Protocol("this server asks for no sign-in".into())),
        s => Err(NetError::Protocol(format!("the server answered {s}"))),
    }
}

/// Sign in to server `url`: `open` shows the sign-in page (the system browser). How to sign in
/// from now on, and what the keychain is to keep.
pub fn sign_in(url: &str, open: &mut dyn FnMut(&str) -> std::result::Result<(), String>, timeout: std::time::Duration) -> Result<(McpOAuth, Kept)> {
    let asked = probe(url)?;
    let (p, registration) = discover(url, &asked)?;
    let Some(endpoint) = registration else {
        return Err(NetError::Auth(format!("{}'s sign-in server doesn't let apps register there, which MailCraft needs to sign in", p.name)));
    };
    let mut registered = |redirect: &str| -> Result<Provider> {
        let (client_id, client_secret) = register(&endpoint, redirect)?;
        Ok(Provider { client_id, client_secret, ..p.clone() })
    };
    let (made, tokens) = oauth::authorize_registering(&mut registered, open, timeout)?;
    let how = McpOAuth {
        issuer: made.issuer.clone().unwrap_or_default(),
        token_url: made.token_url.clone(),
        client_id: made.client_id.clone(),
        resource: made.resource.clone().unwrap_or_default(),
        scope: made.scope.clone(),
    };
    let kept = Kept { access: tokens.access, expires: tokens.expires_at, refresh: tokens.refresh, secret: made.client_secret };
    Ok((how, kept))
}

/// A token for the server `how` is for, from what the keychain keeps: the access token while
/// it's good, else a new one, with what's to be kept now (the refresh token turns over).
pub fn token(how: &McpOAuth, kept: &Kept) -> Result<(String, Option<Kept>)> {
    if kept.expires > oauth::now_unix() {
        return Ok((kept.access.clone(), None));
    }
    let Some(refresh) = &kept.refresh else {
        return Err(NetError::Auth("the sign-in to this server has run out: sign in again in File › Options › AI".into()));
    };
    let p = Provider {
        id: "mcp".into(),
        name: Url::parse(&how.resource).map(|u| u.host).unwrap_or_default(),
        auth_url: String::new(),
        token_url: address(&how.token_url)?,
        scope: how.scope.clone(),
        client_id: how.client_id.clone(),
        client_secret: kept.secret.clone(),
        resource: Some(how.resource.clone()),
        issuer: Some(how.issuer.clone()),
        iss_always: false,
    };
    let t = oauth::refresh(&p, refresh)?;
    let now =
        Kept { access: t.access.clone(), expires: t.expires_at, refresh: t.refresh.or_else(|| Some(refresh.clone())), secret: kept.secret.clone() };
    Ok((t.access, Some(now)))
}

/// At most `most` bytes of `s`, cut at a character's start.
fn short(s: &str, most: usize) -> String {
    let mut end = s.len().min(most);
    while !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    s.get(..end).unwrap_or_default().to_string()
}

/// The fuzzer's: `text` read as a challenge, and as resource and authorization server
/// metadata. How much was found.
#[doc(hidden)]
pub fn read_sign_in(text: &str) -> usize {
    let asked = challenge(text);
    let doc = match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let resource = read_resource(&doc, "https://mcp.example/mcp").map_or(0, |(r, i, s)| r.len().saturating_add(i.len()).saturating_add(s.len()));
    let server = read_server(&doc, "https://auth.example", false).map_or(0, |s| s.auth_url.len());
    let urls = metadata_urls(text).len().saturating_add(server_urls(text).len());
    [asked.metadata.map_or(0, |m| m.len()), asked.scope.map_or(0, |s| s.len()), resource, server, urls].iter().fold(0, |a, b| a.saturating_add(*b))
}
