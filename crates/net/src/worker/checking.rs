//! A new account's sign-in, checked before it's added: its incoming server (IMAP, POP or JMAP)
//! and its outgoing one (SMTP) signed in to as the account will connect, then left. Nothing is
//! kept; what failed is said by server, for the account dialog to show.

use std::time::{Duration, Instant};

use mailcraft_model::{AccountConfig, SignInFailure};

use crate::NetError;
use crate::stream::Trusted;

/// Sign in to `acc`'s servers with `password` (and `smtp_password` for an outgoing server with
/// a sign-in of its own): `None` when they took it, else the first that didn't, and why.
///
/// Each server's answer is logged with how long it took, and the check's start: a check that
/// went on for minutes left nothing to say which server it waited on.
pub(crate) fn check_sign_in(acc: &AccountConfig, password: &str, smtp_password: Option<&str>) -> Option<SignInFailure> {
    log::info!("sign-in check: started");
    let began = Instant::now();
    let checked = incoming(acc, password);
    log::info!("{}", said(false, began.elapsed(), &checked));
    if let Err(e) = checked {
        let server = acc.jmap.clone().or_else(|| acc.pop.as_ref().map(|p| p.host.clone())).unwrap_or_else(|| acc.imap.host.clone());
        return Some(failure(acc, server, false, e));
    }
    // A JMAP account sends through the same server.
    if acc.jmap.is_some() {
        return None;
    }
    let began = Instant::now();
    let checked = outgoing(acc, password, smtp_password);
    log::info!("{}", said(true, began.elapsed(), &checked));
    checked.err().map(|e| failure(acc, acc.smtp.host.clone(), true, e))
}

/// The log's line for one server's answer: which server, how it answered, and how long it took.
/// Never what the server said, which can name the account's address.
fn said(outgoing: bool, took: Duration, checked: &crate::Result<()>) -> String {
    let server = if outgoing { "outgoing" } else { "incoming" };
    let how = match checked {
        Ok(()) => "took the sign-in".to_string(),
        Err(NetError::Auth(_)) => "refused the sign-in".to_string(),
        Err(NetError::Untrusted(_)) => "showed a certificate not trusted".to_string(),
        Err(NetError::Io(e)) => format!("couldn't be reached ({:?})", e.kind()),
        Err(NetError::Tls(_)) => "failed to secure the connection".to_string(),
        Err(_) => "failed".to_string(),
    };
    format!("sign-in check: the {server} server {how} in {:.1} s", took.as_secs_f32())
}

fn incoming(acc: &AccountConfig, password: &str) -> crate::Result<()> {
    let user = acc.login_name();
    if let Some(url) = &acc.jmap {
        return crate::jmap::Client::connect(url, user, password, false).map(drop);
    }
    if acc.pop.is_some() {
        return crate::pop::open(acc, password)?.quit();
    }
    let mut c = crate::imap::Client::connect_trusting(&acc.imap, Trusted::of(acc))?;
    c.login_as(user, password, acc.sign_in)?;
    c.logout();
    Ok(())
}

/// The outgoing server signed in to as sending will: its own user name and password when it has
/// them, else the incoming server's.
fn outgoing(acc: &AccountConfig, password: &str, smtp_password: Option<&str>) -> crate::Result<()> {
    let own = acc.smtp_username.as_deref().map(str::trim).filter(|u| !u.is_empty());
    let (user, password) = match own {
        Some(u) => (u, smtp_password.unwrap_or(password)),
        None => (acc.login_name(), password),
    };
    let sign_in = crate::smtp::SignIn { user, password, oauth: false, method: acc.smtp_sign_in };
    crate::smtp::check_sign_in(&acc.smtp, sign_in, Trusted::of(acc))
}

/// What a refusal is said as: in MailCraft's words where the account's connection would say
/// them (Google's app passwords, Microsoft's refusal of passwords), else as it came.
fn failure(acc: &AccountConfig, server: String, outgoing: bool, e: NetError) -> SignInFailure {
    let said = e.to_string();
    let (error, certificate) = match e {
        NetError::Untrusted(shown) => (said, Some(shown)),
        NetError::Auth(m) if mailcraft_model::google_wants_app_password(&m) => (mailcraft_model::GOOGLE_APP_PASSWORD_NEEDED.to_string(), None),
        NetError::Auth(m) if mailcraft_model::microsoft_password_account(acc) => (format!("{} ({m})", mailcraft_model::MICROSOFT_NO_PASSWORDS), None),
        NetError::Auth(m) => (m, None),
        _ => (said, None),
    };
    SignInFailure { server, outgoing, error, certificate }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    use mailcraft_model::{Security, ServerConfig, SignInMethod};

    use super::*;
    use crate::http::tests::{TEST_SERVER_WAIT, accept_within};

    /// An SMTP server on this computer for one connection that takes, or refuses, any sign-in.
    /// What the client said, once it's gone.
    fn smtp(takes: bool) -> (u16, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let mut log = String::new();
            let Some(s) = accept_within(&l, TEST_SERVER_WAIT) else { return log };
            let mut w = s.try_clone().unwrap();
            let mut r = BufReader::new(s);
            let _ = w.write_all(b"220 test ESMTP\r\n");
            let mut line = String::new();
            while r.read_line(&mut line).unwrap_or(0) > 0 {
                log.push_str(&line);
                let cmd = line.trim_end().to_ascii_uppercase();
                let reply: &[u8] = match () {
                    () if cmd.starts_with("EHLO") => b"250-test\r\n250 AUTH PLAIN LOGIN\r\n",
                    () if cmd.starts_with("AUTH") && takes => b"235 ok\r\n",
                    () if cmd.starts_with("AUTH") => b"535 5.7.8 bad credentials\r\n",
                    () if cmd == "QUIT" => b"221 bye\r\n",
                    () => b"250 ok\r\n",
                };
                if w.write_all(reply).is_err() || cmd == "QUIT" {
                    break;
                }
                line.clear();
            }
            log
        });
        (port, h)
    }

    /// An account on the fake IMAP server (its password is `pw`) and an SMTP server, in the clear
    /// on this computer.
    fn account(imap: u16, smtp: u16) -> AccountConfig {
        let mut a = AccountConfig::guess("zoe@example.org");
        a.imap = ServerConfig { host: "127.0.0.1".into(), port: imap, security: Security::None };
        a.smtp = ServerConfig { host: "127.0.0.1".into(), port: smtp, security: Security::None };
        a
    }

    #[test]
    fn both_servers_taking_it_is_no_failure() {
        let st = crate::fake::State::new();
        let (port, h) = smtp(true);
        assert_eq!(check_sign_in(&account(crate::fake::serve_tcp(&st), port), "pw", None), None);
        let said = h.join().unwrap();
        assert!(said.contains("AUTH") && said.contains("QUIT"), "signed in, and left: {said}");
        assert!(!said.contains("MAIL FROM") && !said.contains("DATA"), "nothing sent: {said}");
    }

    #[test]
    fn a_refusal_names_the_server_that_refused() {
        let st = crate::fake::State::new();
        let imap = crate::fake::serve_tcp(&st);
        let f = check_sign_in(&account(imap, 1), "wrong", None).unwrap();
        assert_eq!((f.server.as_str(), f.outgoing), ("127.0.0.1", false));
        assert!(f.error.contains("wrong password"), "{}", f.error);

        let (port, h) = smtp(false);
        let f = check_sign_in(&account(imap, port), "pw", None).unwrap();
        assert!(f.outgoing && f.error.contains("bad credentials"), "{f:?}");
        h.join().unwrap();

        // Nothing there at all.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let f = check_sign_in(&account(closed, 1), "pw", None).unwrap();
        assert!(!f.outgoing && !f.error.is_empty(), "{f:?}");
    }

    /// Google's refusal when it wants an app password is said in MailCraft's words, as the
    /// account's connection says it.
    #[test]
    fn googles_app_password_refusal_is_said_plainly() {
        let st = crate::fake::State::new();
        st.lock().unwrap().refuse_passwords = Some("[ALERT] Application-specific password required: https://support.google.com/mail/".into());
        let f = check_sign_in(&account(crate::fake::serve_tcp(&st), 1), "pw", None).unwrap();
        assert_eq!(f.error, mailcraft_model::GOOGLE_APP_PASSWORD_NEEDED);
    }

    /// The outgoing server's own user name and password, when it has them; none sent where it
    /// takes mail without a sign-in.
    #[test]
    fn the_outgoing_server_is_signed_in_to_as_sending_does() {
        let st = crate::fake::State::new();
        let imap = crate::fake::serve_tcp(&st);
        let (port, h) = smtp(true);
        let mut a = account(imap, port);
        a.smtp_username = Some("out".into());
        assert_eq!(check_sign_in(&a, "pw", Some("spw")), None);
        let said = h.join().unwrap();
        assert!(said.contains(&mailcraft_model::base64::encode(b"\0out\0spw")), "its own user and password: {said}");

        let (port, h) = smtp(false);
        let mut a = account(imap, port);
        a.smtp_sign_in = SignInMethod::None;
        assert_eq!(check_sign_in(&a, "pw", None), None);
        assert!(!h.join().unwrap().contains("AUTH"));
    }

    /// The log says which server answered how, and how long it took, never with what it said:
    /// a server's refusal can name the account's address.
    #[test]
    fn the_log_says_each_server_and_its_time_not_its_words() {
        let took = Duration::from_millis(83_400);
        assert_eq!(said(false, took, &Ok(())), "sign-in check: the incoming server took the sign-in in 83.4 s");
        let refused = said(true, took, &Err(NetError::Auth("535 no such user someone@example.com".into())));
        assert_eq!(refused, "sign-in check: the outgoing server refused the sign-in in 83.4 s");
        let slow = said(false, took, &Err(NetError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "imap.example.com:993"))));
        assert_eq!(slow, "sign-in check: the incoming server couldn't be reached (TimedOut) in 83.4 s");
        for e in [NetError::Protocol("* BYE someone@example.com".into()), NetError::Closed("someone@example.com".into())] {
            assert!(!said(true, took, &Err(e)).contains('@'), "nothing the server said");
        }
    }
}
