use super::super::tests::socket;
use super::*;

/// A window for the tests: answers each agent's message with it, wrapped, and notifications
/// (`"note"` in them) with nothing, `slow` after taking it, until `stop`.
fn window(server: Server, stop: Arc<AtomicBool>, slow: Duration) -> std::thread::JoinHandle<Server> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            for ask in server.agent_asks() {
                std::thread::sleep(slow);
                let reply = (!ask.message().contains("note")).then(|| format!("{{\"answered\":{}}}", ask.message()));
                ask.answer(reply);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        server
    })
}

/// An agent's connection: each message answered by the window, in turn, on a line of its own;
/// a notification not at all; and later launches still answered while it's connected.
#[test]
fn an_agent_talks_to_the_window_through_the_socket() {
    let path = socket("agent");
    let Claim::Primary(first) = claim_at(&path, &Handed::default()) else { panic!("the first launch listens") };
    let stop = Arc::new(AtomicBool::new(false));
    let running = window(first, Arc::clone(&stop), Duration::ZERO);
    let stream = connect(&path).expect("the agent is let in");
    stream.set_read_timeout(Some(TIMEOUT * 2)).unwrap();
    let mut writer = &stream;
    writer.write_all(b"{\"id\":1}\n{\"note\":true}\n\n{\"id\":2}\n").unwrap();
    let mut reader = BufReader::new(&stream);
    let mut lines = Vec::new();
    for _ in 0..2 {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        lines.push(line);
    }
    assert_eq!(lines, ["{\"answered\":{\"id\":1}}\n", "{\"answered\":{\"id\":2}}\n"]);
    // Another launch meanwhile: shown, as ever.
    assert!(matches!(claim_at(&path, &Handed::default()), Claim::Shown));
    drop(reader);
    drop(stream);
    stop.store(true, Ordering::Release);
    let first = running.join().unwrap();
    assert!(first.take().is_some(), "the launch was noted");
    first.close();
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// An answer that takes its time reaches the agent, as do those after it: the bridge read the
/// window's answers with the moment's timeout left from waiting for the `ok`, and the first that
/// took longer (a hidden window's, which runs ten times a second at most) ended them for good.
#[test]
fn a_slow_answer_reaches_the_agent() {
    let path = socket("agentslow");
    let Claim::Primary(first) = claim_at(&path, &Handed::default()) else { panic!("the first launch listens") };
    let stop = Arc::new(AtomicBool::new(false));
    let running = window(first, Arc::clone(&stop), Duration::from_millis(300));
    let stream = connect(&path).expect("the agent is let in");
    // Read as the bridge does, on a clone of its own, with nothing more set on it.
    let answers = stream.try_clone().unwrap();
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(&answers);
        for _ in 0..2 {
            let _ = said.send(read_message(&mut reader).map_err(|e| e.kind()));
        }
    });
    let mut writer = &stream;
    for id in 1..=2 {
        writer.write_all(format!("{{\"id\":{id}}}\n").as_bytes()).unwrap();
        let answer = heard.recv_timeout(TIMEOUT * 2).expect("an answer or an error, in time");
        assert_eq!(answer, Ok(Some(format!("{{\"answered\":{{\"id\":{id}}}}}\n"))));
    }
    drop(stream);
    stop.store(true, Ordering::Release);
    running.join().unwrap().close();
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Only the key exactly, with `mcp` true, takes a connection for an agent: anything else gets
/// no `ok`, and nothing reaches the window.
#[test]
fn an_agent_needs_the_key() {
    use std::io::Read as _;
    let path = socket("agentkey");
    let Claim::Primary(first) = claim_at(&path, &Handed::default()) else { panic!("the first launch listens") };
    let key = first.key.clone();
    for line in [
        request(""),
        request("wrong"),
        request(&format!("{key}x")),
        format!("{{\"key\":\"{key}\",\"mcp\":1}}\n"),
        format!("{{\"key\":\"{key}\",\"mcp\":\"true\"}}\n"),
        "{\"mcp\":true}\n".to_string(),
    ] {
        let mut s = UnixStream::connect(&path).unwrap();
        s.set_read_timeout(Some(TIMEOUT * 2)).unwrap();
        s.write_all(line.as_bytes()).unwrap();
        s.write_all(b"{\"id\":1}\n").unwrap();
        let mut reply = Vec::new();
        let _ = s.read_to_end(&mut reply);
        assert!(reply.is_empty(), "nothing for {line:?}");
    }
    assert!(first.agent_asks().is_empty());
    assert!(is_request(&request(&key), &key) && !is_request(&request(&key), "") && !is_request("junk", &key));
    first.close();
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// With no MailCraft running, the bridge answers each request with why, and nothing else.
#[test]
fn no_mailcraft_running_is_said_to_each_request() {
    let path = socket("agentnone");
    assert_eq!(connect(&path).err(), Some(NOT_RUNNING));
    let mut input = std::io::Cursor::new(
        b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"initialize\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\nnot json\n"
            .to_vec(),
    );
    let mut output = Vec::new();
    refuse_all(&mut input, &mut output, NOT_RUNNING);
    let said = String::from_utf8(output).unwrap();
    let lines: Vec<serde_json::Value> = said.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 1, "{said}");
    assert_eq!((lines[0]["id"].as_i64(), lines[0]["error"]["message"].as_str()), (Some(7), Some(NOT_RUNNING)));
}

/// A message longer than the most read ends the connection rather than being read into memory.
#[test]
fn a_message_too_long_ends_the_connection() {
    let long = vec![b'x'; usize::try_from(MAX_MESSAGE).unwrap() + 10];
    let mut reader = std::io::Cursor::new(long);
    assert!(read_message(&mut reader).is_err());
    let mut reader = std::io::Cursor::new(b"{}\n{}".to_vec());
    assert_eq!(read_message(&mut reader).unwrap().as_deref(), Some("{}\n"));
    assert_eq!(read_message(&mut reader).unwrap().as_deref(), Some("{}"));
    assert_eq!(read_message(&mut reader).unwrap(), None);
}
