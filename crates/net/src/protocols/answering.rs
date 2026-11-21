//! Which of several mail servers answer, asked all at once: account setup's last guess, when
//! nothing is published for an address's domain ([`crate::autoconfig`] names the servers). Only a
//! server that answers as IMAP or SMTP over TLS or STARTTLS, with a certificate an authority
//! vouches for for its name, counts: nothing is signed in to, and nothing is said but hello.

use std::sync::mpsc;
use std::time::{Duration, Instant};

#[cfg(test)]
use mailcraft_model::Security;
use mailcraft_model::ServerConfig;

use crate::stream::Trusted;

/// How long the servers asked have, together, to answer.
pub const PROBE_WAIT: Duration = Duration::from_secs(10);

/// What to take of servers asked best first, from what each said so far (`None`: not yet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    /// This one answered, and every better one didn't.
    Take(usize),
    /// None answered.
    Nothing,
    /// A better one than any that answered hasn't said yet.
    Waiting,
}

pub(crate) fn pick(answers: &[Option<bool>]) -> Pick {
    for (i, a) in answers.iter().enumerate() {
        match a {
            Some(true) => return Pick::Take(i),
            Some(false) => {}
            None => return Pick::Waiting,
        }
    }
    Pick::Nothing
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Imap,
    Smtp,
}

/// Whether `server` answers as `kind` says, trusting `pinned` besides the authorities.
fn answers(kind: Kind, server: &ServerConfig, pinned: &[String]) -> bool {
    let trusted = Trusted::pinned(pinned);
    match kind {
        Kind::Imap => match crate::imap::Client::connect_trusting(server, trusted) {
            Ok(mut c) => {
                c.logout();
                true
            }
            Err(_) => false,
        },
        Kind::Smtp => crate::smtp::reach(server, trusted).is_ok(),
    }
}

/// The best IMAP and SMTP servers of `imap` and `smtp` (each best first) that answer, given
/// [`PROBE_WAIT`]: what account setup asks.
pub fn answering(imap: &[ServerConfig], smtp: &[ServerConfig]) -> (Option<ServerConfig>, Option<ServerConfig>) {
    probe(imap, smtp, &[], PROBE_WAIT)
}

/// Ask each of `imap` and `smtp` (best first) at once whether it answers, trusting `pinned`
/// besides the authorities (the tests' certificate; none otherwise): the best IMAP and SMTP
/// servers that did, each once that's known, or once `wait` is up (one that hasn't answered by
/// then doesn't count).
pub(crate) fn probe(imap: &[ServerConfig], smtp: &[ServerConfig], pinned: &[String], wait: Duration) -> (Option<ServerConfig>, Option<ServerConfig>) {
    let (tx, rx) = mpsc::channel::<(Kind, usize, bool)>();
    for (kind, list) in [(Kind::Imap, imap), (Kind::Smtp, smtp)] {
        for (i, server) in list.iter().enumerate() {
            let (asked, server, pinned) = (tx.clone(), server.clone(), pinned.to_vec());
            let spawned = std::thread::Builder::new().name("mail-probe".into()).spawn(move || {
                let _ = asked.send((kind, i, answers(kind, &server, &pinned)));
            });
            if let Err(e) = spawned {
                log::warn!("autoconfiguration: {e}");
                let _ = tx.send((kind, i, false));
            }
        }
    }
    drop(tx);
    let mut said = (vec![None; imap.len()], vec![None; smtp.len()]);
    let until = Instant::now() + wait;
    while pick(&said.0) == Pick::Waiting || pick(&said.1) == Pick::Waiting {
        let Ok((kind, i, ok)) = rx.recv_timeout(until.saturating_duration_since(Instant::now())) else { break };
        let list = if kind == Kind::Imap { &mut said.0 } else { &mut said.1 };
        if let Some(a) = list.get_mut(i) {
            *a = Some(ok);
        }
    }
    // What hasn't answered by now didn't.
    let best = |list: &[ServerConfig], said: &[Option<bool>]| said.iter().position(|a| *a == Some(true)).and_then(|i| list.get(i).cloned());
    (best(imap, &said.0), best(smtp, &said.1))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    use rustls::{ServerConnection, StreamOwned};

    use super::*;
    use crate::http::tests::{TEST_SERVER_WAIT, accept_within};
    use crate::stream::trust::fingerprint;
    use crate::stream::trust::test_certs::server_identity;

    const IMAP: &[u8] = b"* OK [CAPABILITY IMAP4rev1] ready\r\n";
    const SMTP: &[u8] = b"220 test ESMTP\r\n";

    /// The greeting, then an answer to each line until the client goes: EHLO and QUIT as SMTP
    /// answers them, anything else as IMAP answers LOGOUT. No greeting: says nothing for a while,
    /// as a server that took the connection and hangs.
    fn talk(s: &mut (impl Read + Write), greeting: &[u8]) {
        if greeting.is_empty() {
            std::thread::sleep(Duration::from_secs(3));
            return;
        }
        if s.write_all(greeting).and_then(|()| s.flush()).is_err() {
            return;
        }
        let mut buf = [0u8; 512];
        while let Ok(n @ 1..) = s.read(&mut buf) {
            let line = String::from_utf8_lossy(&buf[..n]).to_string();
            let reply = match line.to_ascii_uppercase() {
                l if l.starts_with("EHLO") => "250 test\r\n".to_string(),
                l if l.starts_with("QUIT") => "221 bye\r\n".to_string(),
                _ => format!("* BYE bye\r\n{} OK done\r\n", line.split(' ').next().unwrap_or("a")),
            };
            if s.write_all(reply.as_bytes()).and_then(|()| s.flush()).is_err() {
                return;
            }
        }
    }

    /// A server on 127.0.0.1 for one connection: over TLS with the test certificate when `tls`,
    /// else in the clear. Its port.
    fn serve(tls: bool, greeting: &'static [u8]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Some(mut tcp) = accept_within(&listener, TEST_SERVER_WAIT) else { return };
            if !tls {
                return talk(&mut tcp, greeting);
            }
            let (chain, key) = server_identity();
            let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(chain, key)
                .unwrap();
            let Ok(conn) = ServerConnection::new(Arc::new(config)) else { return };
            talk(&mut StreamOwned::new(conn, tcp), greeting);
        });
        port
    }

    /// A port nothing listens on.
    fn closed() -> u16 {
        TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    fn at(port: u16) -> ServerConfig {
        ServerConfig { host: "127.0.0.1".into(), port, security: Security::Tls }
    }

    fn the_certificate() -> Vec<String> {
        vec![fingerprint(&server_identity().0[0])]
    }

    #[test]
    fn the_best_that_answered_once_each_better_one_said() {
        assert_eq!(pick(&[]), Pick::Nothing);
        assert_eq!(pick(&[Some(false), Some(false)]), Pick::Nothing);
        assert_eq!(pick(&[Some(false), Some(true), None]), Pick::Take(1));
        assert_eq!(pick(&[None, Some(true)]), Pick::Waiting);
    }

    /// Of servers that don't listen, answer in the clear where TLS is asked, or answer as they
    /// should, the best of the last; for SMTP as for IMAP. A certificate nobody vouches for
    /// (none trusted by hand) doesn't count.
    #[test]
    fn takes_the_best_server_that_answers_securely() {
        let imap = [at(closed()), at(serve(false, IMAP)), at(serve(true, IMAP)), at(serve(true, IMAP))];
        let smtp = [at(closed()), at(serve(true, SMTP))];
        let (i, s) = probe(&imap, &smtp, &the_certificate(), TEST_SERVER_WAIT);
        assert_eq!(i.as_ref(), imap.get(2));
        assert_eq!(s.as_ref(), smtp.get(1));

        let imap = [at(serve(true, IMAP))];
        let smtp = [at(serve(true, SMTP))];
        assert_eq!(probe(&imap, &smtp, &[], TEST_SERVER_WAIT), (None, None), "its certificate trusted by no one");
    }

    /// A better server that took the connection and says nothing isn't waited for past the
    /// time given: the one that answered is taken then.
    #[test]
    fn a_server_that_hangs_isnt_waited_for() {
        let imap = [at(serve(true, b"")), at(serve(true, IMAP))];
        let started = Instant::now();
        let (i, s) = probe(&imap, &[], &the_certificate(), Duration::from_millis(500));
        assert_eq!((i.as_ref(), s), (imap.get(1), None));
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }
}
