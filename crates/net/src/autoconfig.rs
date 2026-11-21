//! Finding an account's server settings from its address, the way mail clients have agreed to:
//!
//! 1. the domain's own configuration: `https://autoconfig.<domain>/mail/config-v1.1.xml` and
//!    `https://<domain>/.well-known/autoconfig/mail/config-v1.1.xml` (Mozilla's autoconfig
//!    format, the de-facto standard);
//! 2. the public ISP database (`https://autoconfig.thunderbird.net/v1.1/<domain>`);
//! 3. for the domain of its mail exchanger (MX), which covers domains hosted by Google,
//!    Microsoft, Spacemail and the like: the same database, then that provider's own
//!    configuration (`https://autoconfig.<provider>/mail/config-v1.1.xml`);
//! 4. the host an `_autodiscover._tcp` SRV record names (where Outlook looks), for the same
//!    configuration file;
//! 5. DNS SRV records (RFC 6186: `_imaps._tcp`, `_imap._tcp`, `_submissions._tcp`,
//!    `_submission._tcp`);
//! 6. with nothing published, the names most hosts give their servers (`imap.`, `mail.`, the
//!    domain itself, [`candidates`]), asked whether they answer over TLS by the caller's probe
//!    (the network's is in `protocols`, which this module stays apart from).
//!
//! The first answer wins; a POP server offered alongside is kept (`pop`) for POP accounts. A
//! JMAP server the domain's `_jmap._tcp` SRV record names (RFC 8620 §2.2) is kept beside it
//! (`jmap`, offered, not chosen); with only that published, it's the answer (and `imap` has no
//! host) before any server is asked. Only
//! the domain's own server is told the address; the others are asked about the domain, and
//! only when the user types an address into the account dialog. Everything fetched is hostile input: sizes are capped and
//! a bad file is simply skipped.

use mailcraft_model::{AccountConfig, Security, ServerConfig, SignInMethod};

use crate::xml::{self, El};

/// Largest configuration file read.
const MAX_XML: usize = 256 << 10;

/// Settings found for an address, and where they came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub config: AccountConfig,
    pub source: String,
}

/// The text of the first `<tag>` in `block`, trimmed.
fn text_of<'a>(block: &'a El, tag: &str) -> Option<&'a str> {
    block.find(tag).map(|e| e.text.trim())
}

fn server(block: &El, email: &str) -> Option<(ServerConfig, String)> {
    let host = text_of(block, "hostname")?;
    let port: u16 = text_of(block, "port")?.parse().ok()?;
    let security = match text_of(block, "socketType").unwrap_or_default().to_ascii_uppercase().as_str() {
        "SSL" | "TLS" => Security::Tls,
        "STARTTLS" => Security::StartTls,
        _ => Security::None,
    };
    let (local, domain) = email.rsplit_once('@').unwrap_or((email, ""));
    let user = text_of(block, "username").unwrap_or("%EMAILADDRESS%");
    let user = user.replace("%EMAILADDRESS%", email).replace("%EMAILLOCALPART%", local).replace("%EMAILDOMAIN%", domain);
    let host = host.replace("%EMAILDOMAIN%", domain);
    let ok = !host.is_empty() && port != 0 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    ok.then_some((ServerConfig { host: host.to_ascii_lowercase(), port, security }, user))
}

/// How a server's block says it's signed in to (its `<authentication>` elements; none listed:
/// with the password, as the format's own default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Methods {
    /// `password-cleartext` or `password-encrypted` (and their old names `plain`, `secure`).
    password: bool,
    ntlm: bool,
    oauth: bool,
    /// `none` or `client-IP-address`: an outgoing server that takes mail without a sign-in.
    nothing: bool,
}

impl Methods {
    fn of(block: &El) -> Methods {
        let named: Vec<String> = block.children("authentication").map(|a| a.text.trim().to_ascii_lowercase()).collect();
        let has = |names: &[&str]| named.iter().any(|n| names.contains(&n.as_str()));
        Methods {
            password: named.is_empty() || has(&["password-cleartext", "password-encrypted", "plain", "secure"]),
            ntlm: has(&["ntlm"]),
            oauth: has(&["oauth2"]),
            nothing: has(&["none", "client-ip-address"]),
        }
    }

    /// Whether MailCraft can sign in to `host` a way it says (GSSAPI, a client certificate or
    /// another provider's OAuth alone aren't).
    fn usable(self, host: &str) -> bool {
        self.password || self.ntlm || self.nothing || (self.oauth && mailcraft_model::oauth_provider_by_domain(host).is_some())
    }

    /// How MailCraft signs in: NTLM where that's the only way with a password, none where none
    /// is needed (an outgoing server), else as the server offers.
    fn sign_in(self) -> SignInMethod {
        match self {
            Methods { password: false, ntlm: true, .. } => SignInMethod::Ntlm,
            Methods { password: false, ntlm: false, oauth: false, nothing: true } => SignInMethod::None,
            _ => SignInMethod::Automatic,
        }
    }
}

/// The best IMAP and SMTP servers of a Mozilla autoconfig file, and its POP server when it has
/// one (in `pop`). A server MailCraft can sign in to the way it says is preferred, then an
/// encrypted one; one without encryption is only taken when nothing else is offered. How each
/// is signed in to is kept (`sign_in`, `smtp_sign_in`), and, when the IMAP server takes OAuth
/// from a provider MailCraft can sign in with, that provider (`oauth`: offered, not chosen).
pub fn parse_config(xml: &str, email: &str) -> Option<AccountConfig> {
    let doc = xml::parse(xml);
    let pick = |tag: &str, kind: &str| -> Option<(ServerConfig, String, Methods)> {
        let mut blocks = Vec::new();
        doc.all(tag, &mut blocks);
        let mut all: Vec<(ServerConfig, String, Methods)> = blocks
            .into_iter()
            .filter(|b| b.attr("type") == Some(kind))
            .filter_map(|b| server(b, email).map(|(s, user)| (s, user, Methods::of(b))))
            .collect();
        all.sort_by_key(|(s, _, m)| {
            let security = match s.security {
                Security::Tls => 0,
                Security::StartTls => 1,
                Security::None => 2,
            };
            (!m.usable(&s.host), security)
        });
        all.into_iter().next()
    };
    let (imap, user, methods) = pick("incomingServer", "imap")?;
    let (smtp, _, smtp_methods) = pick("outgoingServer", "smtp")?;
    let mut c = AccountConfig::guess(email);
    c.oauth = methods.oauth.then(|| mailcraft_model::oauth_provider_by_domain(&imap.host)).flatten().map(str::to_string);
    c.imap = imap;
    c.smtp = smtp;
    c.username = user;
    c.sign_in = methods.sign_in();
    c.smtp_sign_in = smtp_methods.sign_in();
    c.pop = pick("incomingServer", "pop3").map(|(s, _, _)| s);
    Some(c)
}

/// The registrable part of a host name (`aspmx.l.google.com` → `google.com`; two-letter country
/// domains keep one more label: `mx.example.co.uk` → `example.co.uk`).
pub fn base_domain(host: &str) -> String {
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').filter(|l| !l.is_empty()).collect();
    let n = labels.len();
    let keep = if n >= 3
        && labels.get(n - 1).is_some_and(|t| t.len() == 2)
        && labels.get(n - 2).is_some_and(|s| ["co", "com", "net", "org", "ac", "gov", "edu", "ne", "or"].contains(s))
    {
        3
    } else {
        2
    };
    labels[n.saturating_sub(keep)..].join(".")
}

/// The servers asked whether they answer for `domain`, when it publishes nothing, best first:
/// IMAP's, then SMTP's. Implicit TLS before STARTTLS (RFC 8314), each on `imap.` (or `smtp.`),
/// `mail.` and the domain itself: only the domain's own names, never in the clear.
pub fn candidates(domain: &str) -> (Vec<ServerConfig>, Vec<ServerConfig>) {
    let list = |first: &str, tls: u16, starttls: u16| -> Vec<ServerConfig> {
        [(tls, Security::Tls), (starttls, Security::StartTls)]
            .into_iter()
            .flat_map(|(port, security)| {
                [format!("{first}.{domain}"), format!("mail.{domain}"), domain.to_string()].into_iter().map(move |host| ServerConfig {
                    host,
                    port,
                    security,
                })
            })
            .collect()
    };
    (list("imap", 993, 143), list("smtp", 465, 587))
}

/// Of servers given best first (IMAP's, SMTP's), the best of each that answers.
pub type Probe<'a> = &'a mut dyn FnMut(&[ServerConfig], &[ServerConfig]) -> (Option<ServerConfig>, Option<ServerConfig>);

/// The lookups, injectable for tests.
pub struct Lookups<'a> {
    /// GET a URL; the body when it answered 200.
    pub fetch: &'a mut dyn FnMut(&str) -> Option<String>,
    pub mx: &'a mut dyn FnMut(&str) -> Vec<String>,
    pub srv: &'a mut dyn FnMut(&str) -> Option<(String, u16)>,
    /// The best IMAP and SMTP servers that answer, of those given best first.
    pub probe: Probe<'a>,
}

/// Find settings for `email` (see the module docs for the order).
pub fn discover_with(email: &str, l: &mut Lookups<'_>) -> Option<Found> {
    let email = email.trim();
    let domain = email.rsplit_once('@').map(|(_, d)| d.trim().to_ascii_lowercase()).filter(|d| d.contains('.'))?;
    if !domain.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')) {
        return None;
    }
    let published = published(email, &domain, l);
    let jmap = jmap_session(&domain, l);
    if let Some(mut found) = published {
        found.config.jmap = jmap;
        return Some(found);
    }
    if let Some(url) = jmap {
        let mut c = AccountConfig::guess(email);
        c.imap.host = String::new();
        c.jmap = Some(url);
        return Some(Found { config: c, source: "DNS (a JMAP SRV record)".into() });
    }
    // Nothing published: the domain's own servers under the usual names, if they answer. An
    // outgoing server that didn't is guessed on the incoming one's host.
    let (imap, smtp) = candidates(&domain);
    let (imap, smtp) = (l.probe)(&imap, &smtp);
    let imap = imap?;
    let smtp = smtp.unwrap_or_else(|| ServerConfig { host: imap.host.clone(), port: 465, security: Security::Tls });
    let mut c = AccountConfig::guess(email);
    c.imap = imap;
    c.smtp = smtp;
    Some(Found { config: c, source: format!("{domain}'s servers, which answered") })
}

/// The JMAP session address `domain`'s `_jmap._tcp` SRV record gives (RFC 8620 §2.2):
/// `https://<host>[:<port>]/.well-known/jmap`.
fn jmap_session(domain: &str, l: &mut Lookups<'_>) -> Option<String> {
    let (host, port) = (l.srv)(&format!("_jmap._tcp.{domain}"))?;
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let named = !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    named.then(|| if port == 443 { format!("https://{host}/.well-known/jmap") } else { format!("https://{host}:{port}/.well-known/jmap") })
}

/// Settings published for `domain` (steps 1 to 5 of the module docs).
fn published(email: &str, domain: &str, l: &mut Lookups<'_>) -> Option<Found> {
    let enc = crate::http::pct(email);
    let try_url = |l: &mut Lookups<'_>, url: &str| (l.fetch)(url).and_then(|x| parse_config(&x, email));
    for url in [
        format!("https://autoconfig.{domain}/mail/config-v1.1.xml?emailaddress={enc}"),
        format!("https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml"),
    ] {
        if let Some(c) = try_url(l, &url) {
            return Some(Found { config: c, source: domain.to_string() });
        }
    }
    if let Some(c) = try_url(l, &format!("https://autoconfig.thunderbird.net/v1.1/{domain}")) {
        return Some(Found { config: c, source: "the ISP database".into() });
    }
    if let Some(mx) = (l.mx)(domain).into_iter().next() {
        let base = base_domain(&mx);
        if base != domain {
            if let Some(c) = try_url(l, &format!("https://autoconfig.thunderbird.net/v1.1/{base}")) {
                return Some(Found { config: c, source: format!("the ISP database ({base} handles this domain's mail)") });
            }
            // The mail provider's own configuration (asked about the domain, not the address).
            if let Some(c) = try_url(l, &format!("https://autoconfig.{base}/mail/config-v1.1.xml?emaildomain={domain}")) {
                return Some(Found { config: c, source: base });
            }
        }
    }
    // Where Outlook looks: the host an `_autodiscover._tcp` record names, which providers point
    // at their configuration server.
    if let Some((host, _)) = (l.srv)(&format!("_autodiscover._tcp.{domain}"))
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        && let Some(c) = try_url(l, &format!("https://{host}/mail/config-v1.1.xml?emaildomain={domain}"))
    {
        return Some(Found { config: c, source: base_domain(&host) });
    }
    let imap = (l.srv)(&format!("_imaps._tcp.{domain}"))
        .map(|(h, p)| ServerConfig { host: h, port: p, security: Security::Tls })
        .or_else(|| (l.srv)(&format!("_imap._tcp.{domain}")).map(|(h, p)| ServerConfig { host: h, port: p, security: Security::StartTls }));
    let smtp = (l.srv)(&format!("_submissions._tcp.{domain}"))
        .map(|(h, p)| ServerConfig { host: h, port: p, security: Security::Tls })
        .or_else(|| (l.srv)(&format!("_submission._tcp.{domain}")).map(|(h, p)| ServerConfig { host: h, port: p, security: Security::StartTls }));
    let (imap, smtp) = (imap?, smtp?);
    let mut c = AccountConfig::guess(email);
    c.imap = imap;
    c.smtp = smtp;
    Some(Found { config: c, source: "DNS (SRV records)".into() })
}

/// Find settings for `email` over the network, `probe` asking servers whether they answer
/// (`mailcraft_net::answering::answering`).
pub fn discover(email: &str, probe: Probe<'_>) -> Option<Found> {
    let mut fetch = |url: &str| {
        crate::http::get(url, MAX_XML)
            .ok()
            .filter(|r| r.ok() && r.header("content-type").is_none_or(|t| t.contains("xml") || t.contains("text")))
            .map(|r| String::from_utf8_lossy(&r.body).to_string())
    };
    let mut mx = crate::dns::mx;
    let mut srv = crate::dns::srv;
    discover_with(email, &mut Lookups { fetch: &mut fetch, mx: &mut mx, srv: &mut srv, probe })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A probe of servers that finds none answering.
    fn no_probe(_: &[ServerConfig], _: &[ServerConfig]) -> (Option<ServerConfig>, Option<ServerConfig>) {
        (None, None)
    }

    const XML: &str = r#"<?xml version="1.0"?>
<clientConfig version="1.1">
  <emailProvider id="example.org">
    <incomingServer type="pop3"><hostname>pop.example.org</hostname><port>995</port><socketType>SSL</socketType></incomingServer>
    <incomingServer type="imap"><hostname>plain.example.org</hostname><port>143</port><socketType>plain</socketType><username>%EMAILADDRESS%</username></incomingServer>
    <incomingServer type='imap'>
      <hostname>imap.example.org</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <username>%EMAILLOCALPART%</username>
    </incomingServer>
    <outgoingServer type="smtp"><hostname>smtp.%EMAILDOMAIN%</hostname><port>587</port><socketType>STARTTLS</socketType></outgoingServer>
  </emailProvider>
</clientConfig>"#;

    #[test]
    fn reads_mozilla_autoconfig() {
        let c = parse_config(XML, "zoe@example.org").unwrap();
        assert_eq!(c.imap, ServerConfig { host: "imap.example.org".into(), port: 993, security: Security::Tls }, "encrypted IMAP wins");
        assert_eq!(c.smtp, ServerConfig { host: "smtp.example.org".into(), port: 587, security: Security::StartTls });
        assert_eq!(c.username, "zoe");
        assert!(parse_config("<html>nope</html>", "a@b.c").is_none());
        assert!(
            parse_config(&XML.replace("<port>993</port>", "<port>x</port>").replace("<port>143</port>", "<port>0</port>"), "z@example.org").is_none()
        );
        let evil = XML.replace("imap.example.org", "imap.example.org/../x");
        assert_eq!(parse_config(&evil, "z@example.org").unwrap().imap.host, "plain.example.org", "bad host names are skipped");
    }

    /// How each server is signed in to, as its `<authentication>` says: NTLM where that's the only
    /// way, no sign-in for an outgoing server that takes mail from where it's sent, a server
    /// MailCraft can't sign in to the way it says passed over for one it can, and OAuth offered
    /// only by a provider MailCraft signs in with.
    #[test]
    fn reads_how_each_server_is_signed_in_to() {
        let file = |imap: &str, smtp: &str| {
            format!(
                r#"<clientConfig><emailProvider id="example.org">
                <incomingServer type="imap"><hostname>{imap}</hostname><port>993</port><socketType>SSL</socketType><authentication>GSSAPI</authentication></incomingServer>
                <incomingServer type="imap"><hostname>{imap}</hostname><port>143</port><socketType>STARTTLS</socketType>
                  <authentication>NTLM</authentication></incomingServer>
                <outgoingServer type="smtp"><hostname>{smtp}</hostname><port>25</port><socketType>STARTTLS</socketType>
                  <authentication>client-IP-address</authentication></outgoingServer>
                </emailProvider></clientConfig>"#
            )
        };
        let c = parse_config(&file("imap.example.org", "smtp.example.org"), "zoe@example.org").unwrap();
        assert_eq!((c.imap.port, c.sign_in), (143, SignInMethod::Ntlm), "GSSAPI alone passed over, encrypted or not");
        assert_eq!(c.smtp_sign_in, SignInMethod::None);
        assert_eq!(c.oauth, None);
        // None listed: the password, as before.
        let c = parse_config(XML, "zoe@example.org").unwrap();
        assert_eq!((c.sign_in, c.smtp_sign_in, c.oauth.as_deref()), (SignInMethod::Automatic, SignInMethod::Automatic, None));

        let google = r#"<clientConfig><emailProvider id="example.org">
            <incomingServer type="imap"><hostname>imap.gmail.com</hostname><port>993</port><socketType>SSL</socketType>
              <authentication>OAuth2</authentication><authentication>password-cleartext</authentication></incomingServer>
            <outgoingServer type="smtp"><hostname>smtp.gmail.com</hostname><port>465</port><socketType>SSL</socketType>
              <authentication>OAuth2</authentication><authentication>password-cleartext</authentication></outgoingServer>
            </emailProvider></clientConfig>"#;
        let c = parse_config(google, "zoe@example.org").unwrap();
        assert_eq!((c.oauth.as_deref(), c.sign_in, c.smtp_sign_in), (Some("google"), SignInMethod::Automatic, SignInMethod::Automatic));
        // Another provider's OAuth isn't offered: MailCraft can't sign in with it.
        let other = google.replace("gmail.com", "mail.example.net");
        assert_eq!(parse_config(&other, "zoe@example.org").unwrap().oauth, None);
        // Nor Google's for a server only named like it.
        let lookalike = google.replace("imap.gmail.com", "imap.gmail.com.example.net");
        assert_eq!(parse_config(&lookalike, "zoe@example.org").unwrap().oauth, None);
    }

    /// A JMAP server the domain's SRV record names is kept beside the settings found (Fastmail's
    /// way), and is the answer, before any server is asked, when it's all that's published.
    #[test]
    fn a_jmap_server_named_in_dns_is_found() {
        let mut no_mx = |_: &str| Vec::new();
        let mut srv = |name: &str| match name {
            "_jmap._tcp.studio.example" => Some(("API.Mailhost.example.".to_string(), 443)),
            "_jmap._tcp.other.example" => Some(("jmap.other.example".to_string(), 8443)),
            _ => None,
        };
        let mut fetch = |url: &str| url.starts_with("https://autoconfig.studio.example/").then(|| XML.to_string());
        let f = discover_with("zoe@studio.example", &mut Lookups { fetch: &mut fetch, mx: &mut no_mx, srv: &mut srv, probe: &mut no_probe }).unwrap();
        assert_eq!((f.config.imap.host.as_str(), f.source.as_str()), ("imap.example.org", "studio.example"));
        assert_eq!(f.config.jmap.as_deref(), Some("https://api.mailhost.example/.well-known/jmap"));

        let mut none = |_: &str| None;
        let mut probed = false;
        let mut probe = |_: &[ServerConfig], _: &[ServerConfig]| {
            probed = true;
            (None, None)
        };
        let f = discover_with("zoe@other.example", &mut Lookups { fetch: &mut none, mx: &mut no_mx, srv: &mut srv, probe: &mut probe }).unwrap();
        assert_eq!(f.config.jmap.as_deref(), Some("https://jmap.other.example:8443/.well-known/jmap"));
        assert!(f.config.imap.host.is_empty(), "no IMAP server named");
        assert!(!probed);

        // A target that isn't a host name isn't taken.
        let mut bad = |name: &str| name.starts_with("_jmap.").then(|| ("evil.example/x?".to_string(), 443));
        let mut fetch = |url: &str| url.starts_with("https://autoconfig.studio.example/").then(|| XML.to_string());
        let f = discover_with("zoe@studio.example", &mut Lookups { fetch: &mut fetch, mx: &mut no_mx, srv: &mut bad, probe: &mut no_probe }).unwrap();
        assert_eq!(f.config.jmap, None);
    }

    /// A provider's file is read as XML: a server in a comment isn't offered, and character
    /// references and CDATA are read as such.
    #[test]
    fn reads_comments_references_and_cdata_as_xml() {
        const FILE: &str = r#"<clientConfig version="1.1"><emailProvider id="example.org">
            <!-- Retired: <incomingServer type="imap"><hostname>old.example.org</hostname><port>993</port><socketType>SSL</socketType></incomingServer> -->
            <incomingServer type="imap"><hostname><![CDATA[imap.example.org]]></hostname><port>&#57;93</port><socketType>SSL</socketType><username>&#37;EMAILLOCALPART&#x25;</username></incomingServer>
            <outgoingServer type="smtp"><hostname>smtp.example.org</hostname><port>465</port><socketType>SSL</socketType></outgoingServer>
        </emailProvider></clientConfig>"#;
        let c = parse_config(FILE, "zoe@example.org").unwrap();
        assert_eq!(c.imap, ServerConfig { host: "imap.example.org".into(), port: 993, security: Security::Tls });
        assert_eq!(c.username, "zoe");
    }

    /// Live: real DNS and the real ISP database (only domain names are sent). Run by hand with
    /// `cargo test -p mailcraft-net -- --ignored live_`.
    #[test]
    #[ignore = "needs the internet"]
    fn live_discovery() {
        assert!(!crate::dns::mx("gmail.com").is_empty(), "MX lookup through the system resolver");
        let f = discover("someone@gmail.com", &mut no_probe).unwrap();
        assert_eq!(f.config.imap.host, "imap.gmail.com");
        println!("gmail.com: {} ({}); smtp {}:{}", f.config.imap.host, f.source, f.config.smtp.host, f.config.smtp.port);
        assert_eq!((f.config.oauth.as_deref(), f.config.jmap.as_deref()), (Some("google"), None), "its settings say Google's sign-in");
        // Fastmail names its JMAP server in DNS.
        let f = discover("someone@fastmail.com", &mut no_probe).unwrap();
        assert_eq!(f.config.jmap.as_deref(), Some("https://api.fastmail.com/.well-known/jmap"), "{f:?}");
    }

    /// A domain hosted by a provider that publishes its own configuration (not in the ISP
    /// database) and points Outlook at it with an `_autodiscover._tcp` record.
    #[test]
    fn hosted_domains_find_their_provider() {
        const HOSTED: &str = r#"<clientConfig version="1.1"><emailProvider id="mailhost.example">
            <incomingServer type="imap"><hostname>mail.mailhost.example</hostname><port>993</port><socketType>SSL</socketType><username>%EMAILADDRESS%</username></incomingServer>
            <incomingServer type="pop3"><hostname>mail.mailhost.example</hostname><port>995</port><socketType>SSL</socketType><username>%EMAILADDRESS%</username></incomingServer>
            <outgoingServer type="smtp"><hostname>mail.mailhost.example</hostname><port>587</port><socketType>STARTTLS</socketType></outgoingServer>
            <outgoingServer type="smtp"><hostname>mail.mailhost.example</hostname><port>465</port><socketType>SSL</socketType></outgoingServer>
        </emailProvider></clientConfig>"#;
        // Through the mail exchanger's provider.
        let mut asked = Vec::new();
        let mut fetch = |url: &str| {
            asked.push(url.to_string());
            url.starts_with("https://autoconfig.mailhost.example/").then(|| HOSTED.to_string())
        };
        let mut mx = |_: &str| vec!["mx1.mailhost.example".to_string()];
        let mut srv = |_: &str| None;
        let f = discover_with("zoe@studio.example", &mut Lookups { fetch: &mut fetch, mx: &mut mx, srv: &mut srv, probe: &mut no_probe }).unwrap();
        assert_eq!(f.source, "mailhost.example");
        assert_eq!((f.config.imap.host.as_str(), f.config.smtp.port, f.config.smtp.security), ("mail.mailhost.example", 465, Security::Tls));
        assert_eq!(f.config.pop, Some(ServerConfig { host: "mail.mailhost.example".into(), port: 995, security: Security::Tls }));
        assert_eq!(f.config.username, "zoe@studio.example");
        assert!(
            asked.iter().filter(|u| !u.contains("autoconfig.studio.example")).all(|u| !u.contains("zoe")),
            "only the domain's own server is told the address"
        );
        // Through the `_autodiscover._tcp` record, when the mail exchanger says nothing.
        let mut fetch = |url: &str| url.starts_with("https://autoconfig.mailhost.example/").then(|| HOSTED.to_string());
        let mut no_mx = |_: &str| Vec::new();
        let mut srv = |name: &str| (name == "_autodiscover._tcp.studio.example").then(|| ("autoconfig.mailhost.example".to_string(), 443));
        let f = discover_with("zoe@studio.example", &mut Lookups { fetch: &mut fetch, mx: &mut no_mx, srv: &mut srv, probe: &mut no_probe }).unwrap();
        assert_eq!((f.source.as_str(), f.config.pop.map(|p| p.port)), ("mailhost.example", Some(995)));
        // A record naming something that isn't a host name is ignored.
        let mut fetch = |_: &str| None;
        let mut srv = |name: &str| name.starts_with("_autodiscover").then(|| ("evil/../x".to_string(), 443));
        assert!(
            discover_with("zoe@studio.example", &mut Lookups { fetch: &mut fetch, mx: &mut no_mx, srv: &mut srv, probe: &mut no_probe }).is_none()
        );
    }

    /// With nothing published, the domain's own servers under the usual names are asked last:
    /// what answers is taken, the outgoing server guessed on the incoming one's host when none
    /// of those answered. Nothing answering, nothing is found.
    #[test]
    fn nothing_published_asks_the_domains_own_servers_last() {
        let mut none = |_: &str| None;
        let mut no_mx = |_: &str| Vec::new();
        let mut no_srv = |_: &str| None;
        let mut asked: Vec<String> = Vec::new();
        let mut probe = |imap: &[ServerConfig], smtp: &[ServerConfig]| {
            asked.extend(imap.iter().chain(smtp).map(|s| s.host.clone()));
            (imap.get(1).cloned(), None)
        };
        let f = discover_with("zoe@studio.example", &mut Lookups { fetch: &mut none, mx: &mut no_mx, srv: &mut no_srv, probe: &mut probe }).unwrap();
        assert!(!asked.is_empty() && asked.iter().all(|h| h == "studio.example" || h.ends_with(".studio.example")), "{asked:?}");
        assert_eq!(f.config.imap, ServerConfig { host: "mail.studio.example".into(), port: 993, security: Security::Tls });
        assert_eq!(f.config.smtp, ServerConfig { host: "mail.studio.example".into(), port: 465, security: Security::Tls });
        assert_eq!((f.config.username.as_str(), f.source.as_str()), ("zoe@studio.example", "studio.example's servers, which answered"));

        // Published settings are taken without asking any server.
        let mut fetch = |url: &str| url.starts_with("https://autoconfig.studio.example/").then(|| XML.to_string());
        let mut probed = false;
        let mut probe = |_: &[ServerConfig], _: &[ServerConfig]| {
            probed = true;
            (None, None)
        };
        assert!(
            discover_with("zoe@studio.example", &mut Lookups { fetch: &mut fetch, mx: &mut no_mx, srv: &mut no_srv, probe: &mut probe }).is_some()
        );
        assert!(!probed);

        // Only an outgoing server answering isn't enough.
        let mut probe = |_: &[ServerConfig], smtp: &[ServerConfig]| (None, smtp.first().cloned());
        assert!(
            discover_with("zoe@studio.example", &mut Lookups { fetch: &mut none, mx: &mut no_mx, srv: &mut no_srv, probe: &mut probe }).is_none()
        );
    }

    #[test]
    fn asks_the_domains_own_names_encrypted_best_first() {
        let (imap, smtp) = candidates("example.org");
        let named = |l: &[ServerConfig]| l.iter().map(|s| format!("{}:{}", s.host, s.port)).collect::<Vec<_>>();
        assert_eq!(
            named(&imap),
            ["imap.example.org:993", "mail.example.org:993", "example.org:993", "imap.example.org:143", "mail.example.org:143", "example.org:143"]
        );
        assert_eq!(
            named(&smtp),
            ["smtp.example.org:465", "mail.example.org:465", "example.org:465", "smtp.example.org:587", "mail.example.org:587", "example.org:587"]
        );
        assert!(imap.iter().chain(&smtp).all(|s| s.host == "example.org" || s.host.ends_with(".example.org")), "only the domain's own");
        let tls = |l: &[ServerConfig]| l.iter().map(|s| s.security).collect::<Vec<_>>();
        assert_eq!(tls(&imap), [[Security::Tls; 3], [Security::StartTls; 3]].concat(), "never in the clear");
        assert_eq!(tls(&smtp), tls(&imap));
    }

    #[test]
    fn lookups_in_order() {
        let mut asked: Vec<String> = Vec::new();
        let mut fetch = |url: &str| {
            asked.push(url.to_string());
            (url == "https://autoconfig.thunderbird.net/v1.1/google.com").then(|| XML.to_string())
        };
        let mut mx = |d: &str| if d == "studio.example" { vec!["aspmx.l.google.com".to_string()] } else { Vec::new() };
        let mut srv = |_: &str| None;
        let f = discover_with("zoe@studio.example", &mut Lookups { fetch: &mut fetch, mx: &mut mx, srv: &mut srv, probe: &mut no_probe }).unwrap();
        assert!(f.source.contains("google.com"), "{}", f.source);
        assert_eq!(
            asked,
            vec![
                "https://autoconfig.studio.example/mail/config-v1.1.xml?emailaddress=zoe%40studio.example".to_string(),
                "https://studio.example/.well-known/autoconfig/mail/config-v1.1.xml".into(),
                "https://autoconfig.thunderbird.net/v1.1/studio.example".into(),
                "https://autoconfig.thunderbird.net/v1.1/google.com".into(),
            ]
        );

        // SRV when nothing else answers.
        let mut none = |_: &str| None;
        let mut no_mx = |_: &str| Vec::new();
        let mut srv = |name: &str| match name {
            "_imaps._tcp.x.example" => Some(("mail.x.example".to_string(), 993)),
            "_submission._tcp.x.example" => Some(("mail.x.example".to_string(), 587)),
            _ => None,
        };
        let f = discover_with("a@x.example", &mut Lookups { fetch: &mut none, mx: &mut no_mx, srv: &mut srv, probe: &mut no_probe }).unwrap();
        assert_eq!((f.config.imap.port, f.config.imap.security, f.config.smtp.security), (993, Security::Tls, Security::StartTls));
        let mut srv_none = |_: &str| None;
        assert!(discover_with("nobody", &mut Lookups { fetch: &mut none, mx: &mut no_mx, srv: &mut srv_none, probe: &mut no_probe }).is_none());
        assert_eq!(base_domain("aspmx.l.google.com"), "google.com");
        assert_eq!(base_domain("mx1.mail.example.co.uk."), "example.co.uk");
    }
}
