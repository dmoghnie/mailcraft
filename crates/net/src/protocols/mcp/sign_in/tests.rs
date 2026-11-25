use std::time::Duration;

use super::*;
use crate::fakes::fake_mcp::McpServer;
use crate::mcp::McpConn;
use mailcraft_model::McpTarget;

/// The "browser": what the sign-in page asks, followed to the redirect on this computer, on a
/// thread of its own (the sign-in waits for it).
fn browser(url: &str) -> std::result::Result<(), String> {
    let url = url.to_string();
    std::thread::spawn(move || {
        let _ = crate::http::get(&url, 10_000);
    });
    Ok(())
}

/// A challenge's Bearer parameters are read, quoted or not, escapes and all; another scheme's
/// are not, and junk gives nothing.
#[test]
fn a_challenge_says_where_the_metadata_is_and_what_to_ask_for() {
    let c = challenge(
        r#"Bearer error="invalid_token", resource_metadata="https://mcp.example/.well-known/oauth-protected-resource/mcp", scope="notes.read notes.write""#,
    );
    assert_eq!(c.metadata.as_deref(), Some("https://mcp.example/.well-known/oauth-protected-resource/mcp"));
    assert_eq!(c.scope.as_deref(), Some("notes.read notes.write"));
    let c = challenge(r#"Basic realm="x", resource_metadata="https://not.this", Bearer scope=read, realm="a\"b""#);
    assert_eq!((c.metadata, c.scope.as_deref()), (None, Some("read")));
    for junk in ["", "Bearer", "====", "Bearer ,,, =", "\"", "Bearer scope=\"never closed", &"x=".repeat(5000)] {
        let c = challenge(junk);
        assert!(c.metadata.is_none(), "{junk}");
    }
}

/// A resource is the server's address in one form; its metadata and its authorization
/// server's are looked for where MCP says, in its order.
#[test]
fn addresses_are_formed_as_mcp_says() {
    assert_eq!(resource_of("HTTPS://MCP.Example.com:443/Notes/mcp/?q=1#top").unwrap(), "https://mcp.example.com/Notes/mcp");
    assert_eq!(resource_of("http://127.0.0.1:8080/").unwrap(), "http://127.0.0.1:8080");
    assert!(resource_of("http://mcp.example/mcp").is_err(), "plain http elsewhere than this computer");
    assert_eq!(
        metadata_urls("https://mcp.example/public/mcp"),
        ["https://mcp.example/.well-known/oauth-protected-resource/public/mcp", "https://mcp.example/.well-known/oauth-protected-resource"]
    );
    assert_eq!(metadata_urls("https://mcp.example"), ["https://mcp.example/.well-known/oauth-protected-resource"]);
    assert_eq!(
        server_urls("https://auth.example/tenant1"),
        [
            "https://auth.example/.well-known/oauth-authorization-server/tenant1",
            "https://auth.example/.well-known/openid-configuration/tenant1",
            "https://auth.example/tenant1/.well-known/openid-configuration",
        ]
    );
    assert_eq!(
        server_urls("https://auth.example/"),
        ["https://auth.example/.well-known/oauth-authorization-server", "https://auth.example/.well-known/openid-configuration"]
    );
}

/// Metadata for another address, an authorization server naming another, one without PKCE's
/// S256, one whose sign-in page is plain http elsewhere and, for a server elsewhere, one whose
/// addresses are on this computer are refused; a server's sign-in found on this computer, when
/// it's elsewhere, isn't asked for.
#[test]
fn metadata_that_doesnt_fit_is_refused() {
    let doc = |v: Value| match v {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let ours = "https://mcp.example/notes/mcp";
    let read = |v: Value| read_resource(&doc(v), ours);
    let (resource, issuer, scopes) =
        read(json!({"resource": "https://MCP.example/notes", "authorization_servers": ["https://auth.example"], "scopes_supported": ["a", 3]}))
            .unwrap();
    assert_eq!((resource.as_str(), issuer.as_str(), scopes), ("https://mcp.example/notes", "https://auth.example", vec!["a".to_string()]));
    for wrong in [
        json!({"resource": "https://mcp.example/notesx", "authorization_servers": ["a"]}),
        json!({"resource": "https://other.example/notes/mcp", "authorization_servers": ["a"]}),
        json!({"resource": ours}),
    ] {
        assert!(read(wrong.clone()).is_err(), "{wrong}");
    }
    let good = json!({"issuer": "https://auth.example", "authorization_endpoint": "https://auth.example/a", "token_endpoint": "https://auth.example/t", "code_challenge_methods_supported": ["S256"]});
    assert!(read_server(&doc(good.clone()), "https://auth.example", false).is_ok_and(|s| s.registration.is_none() && !s.iss_always));
    let with = |k: &str, v: Value| {
        let mut d = doc(good.clone());
        d.insert(k.into(), v);
        read_server(&d, "https://auth.example", false)
    };
    for (k, v) in [
        ("issuer", json!("https://auth.example/")),
        ("code_challenge_methods_supported", json!(["plain"])),
        ("authorization_endpoint", json!("http://auth.example/a")),
        ("token_endpoint", json!("http://127.0.0.1:8080/t")),
        ("registration_endpoint", json!("https://localhost/r")),
    ] {
        assert!(with(k, v.clone()).is_err(), "{k}: {v}");
    }
    let mut here = doc(good);
    here.insert("token_endpoint".into(), json!("http://127.0.0.1:8080/t"));
    assert!(read_server(&here, "https://auth.example", true).is_ok(), "a server on this computer may sign in on it");
    let elsewhere = Challenge { metadata: Some("http://127.0.0.1:9/.well-known/oauth-protected-resource".into()), scope: None };
    let named = discover("https://mcp.example/mcp", &elsewhere);
    assert!(matches!(&named, Err(NetError::Protocol(m)) if m.contains("on this computer")), "{named:?}");
}

/// MailCraft signs in to a server that asks: it registers, the sign-in page gets PKCE and the
/// resource, the token is for the server alone and reaches it; and when the access token runs
/// out, the refresh token gets another, turning over.
#[test]
fn a_server_that_asks_is_signed_in_to_and_its_token_refreshed() {
    let server = McpServer::start_signed(false);
    let (how, kept) = sign_in(&server.url, &mut browser, Duration::from_secs(10)).unwrap();
    assert_eq!((how.client_id.as_str(), how.resource.as_str(), how.scope.as_str()), ("c-1", server.url.as_str(), "notes.read"));
    assert_eq!((kept.access.as_str(), kept.refresh.as_deref(), kept.secret.as_deref()), ("a-1", Some("r-1"), None));
    let seen: Vec<Value> = server.seen().into_iter().map(|s| s.body).collect();
    let said =
        |target: &str| seen.iter().find(|s| s["target"].as_str().is_some_and(|t| t.starts_with(target))).map(|s| s.to_string()).unwrap_or_default();
    assert!(
        said("/auth/register").contains("127.0.0.1") && said("/auth/register").contains(r#"\"token_endpoint_auth_method\":\"none\""#),
        "{}",
        said("/auth/register")
    );
    assert!(
        said("/auth/authorize").contains("code_challenge_method=S256") && said("/auth/authorize").contains("resource=http"),
        "{}",
        said("/auth/authorize")
    );
    assert!(said("/auth/token").contains("resource=http") && said("/auth/token").contains("code_verifier="), "{}", said("/auth/token"));
    let target = McpTarget::Web { url: server.url.clone(), oauth: Some(how.clone()) };
    let mut c = McpConn::connect(&target, Some(&kept.access)).unwrap();
    assert!(!c.tools().unwrap().is_empty());
    assert_eq!(token(&how, &kept).unwrap(), ("a-1".to_string(), None), "still good: used as it is");
    let (fresh, now) = token(&how, &Kept { expires: 0, ..kept.clone() }).unwrap();
    assert_eq!((fresh.as_str(), now.and_then(|k| k.refresh).as_deref()), ("a-2", Some("r-2")));
    assert!(matches!(token(&how, &Kept { expires: 0, refresh: None, ..kept }), Err(NetError::Auth(_))));
}

/// An answer that names another authorization server is a mix-up: refused, nothing taken from
/// it. A server that asks for no sign-in says so.
#[test]
fn a_mixed_up_answer_or_a_server_that_asks_nothing_is_refused() {
    let server = McpServer::start_signed(true);
    let refused = sign_in(&server.url, &mut browser, Duration::from_secs(10));
    assert!(matches!(&refused, Err(NetError::Auth(m)) if m.contains("didn't come from")), "{refused:?}");
    assert!(!server.seen().iter().any(|s| s.body["target"] == "/auth/token"), "no code went to the token address");
    let open = McpServer::start(false);
    let asked = sign_in(&open.url, &mut browser, Duration::from_secs(10));
    assert!(matches!(&asked, Err(NetError::Protocol(m)) if m.contains("asks for no sign-in")), "{asked:?}");
}

/// What a keychain holds is read back as it was kept; a key typed in by hand isn't taken for
/// a sign-in; no token shows in its `Debug`.
#[test]
fn what_the_keychain_keeps_is_read_back() {
    let k = Kept { access: "a".into(), expires: 5, refresh: Some("r".into()), secret: None };
    assert_eq!(Kept::read(&k.text()), Some(k));
    assert_eq!(Kept::read("sk-typed-by-hand"), None);
    let shown = format!("{:?}", Kept { access: "tok-a".into(), expires: 5, refresh: Some("tok-r".into()), secret: Some("tok-s".into()) });
    assert!(!shown.contains("tok-"), "no token in its Debug: {shown}");
}
