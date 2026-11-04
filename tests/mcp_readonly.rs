//! Every tool through the real binary against a stub portal on 127.0.0.1, so what
//! goes over the wire is what is checked: GETs to the read endpoints, and never a
//! password. Offline and keychain-free: the token comes from `$CPMFL_TOKEN`, and
//! the config points every service at the stub.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

/// Everything a tool may read, by method and path. A new read must be added here
/// on purpose, and nothing but a GET can be.
const READS: [&str; 7] = [
    "GET /pay/account",
    "GET /pay/Ledger",
    "GET /pay/Payment",
    "GET /pay/Charge",
    "GET /actionitems/HomeownerRequests",
    "GET /associations/associations/100/Directories",
    "GET /associations/associations/100/Documents/Search",
];

const TOKEN: &str = "dummy-token";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// What the portal answers: the fixture on a list's first page, an empty page
/// after it, since the fixtures report more pages than they hold.
fn answer(path: &str, query: &str) -> (u16, String) {
    if query
        .split('&')
        .any(|q| q.starts_with("page=") && q != "page=1")
    {
        return (200, r#"{"member": [], "documents": []}"#.into());
    }
    match path {
        "/pay/account" => (200, fixture("account.json")),
        "/pay/Ledger" => (200, fixture("ledger.json")),
        "/pay/Payment" => (200, fixture("payments.json")),
        "/pay/Charge" => (200, fixture("charges.json")),
        "/actionitems/HomeownerRequests" => (200, fixture("homeowner_requests.json")),
        "/associations/associations/100/Directories" => (200, fixture("directories.json")),
        "/associations/associations/100/Documents/Search" => (200, fixture("document_search.json")),
        _ => (404, "{}".into()),
    }
}

#[derive(Clone, Default)]
struct Stub {
    /// `METHOD /path` and the Authorization header of every request seen.
    seen: Arc<Mutex<Vec<(String, String)>>>,
}

impl Stub {
    /// Serve on a free port, answering each request with `answer`, until the
    /// process ends.
    fn start(answer: fn(&str, &str) -> (u16, String)) -> (Self, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stub = Stub::default();
        let seen = stub.seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().map_while(Result::ok) {
                handle(stream, &seen, answer);
            }
        });
        (stub, port)
    }

    fn seen(&self) -> Vec<(String, String)> {
        self.seen.lock().unwrap().clone()
    }
}

fn handle(
    mut stream: TcpStream,
    seen: &Mutex<Vec<(String, String)>>,
    answer: fn(&str, &str) -> (u16, String),
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let mut authorization = String::new();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 || line.trim().is_empty() {
            break;
        }
        let lower = line.to_lowercase();
        if let Some(value) = lower.strip_prefix("authorization:") {
            authorization = line[line.len() - value.len()..].trim().to_string();
        }
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    seen.lock()
        .unwrap()
        .push((format!("{method} {path}"), authorization));
    let (status, text) = answer(path, query);
    write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{text}",
        text.len()
    )
    .unwrap();
}

static DIRS: AtomicU32 = AtomicU32::new(0);

fn config_home(port: u16) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cpmfl-mcp-readonly-{}-{}",
        std::process::id(),
        DIRS.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(dir.join("cpmfl")).unwrap();
    let base = format!("http://127.0.0.1:{port}");
    std::fs::write(
        dir.join("cpmfl/config.json"),
        json!({
            "users_url": format!("{base}/users"),
            "associations_url": format!("{base}/associations"),
            "actionitems_url": format!("{base}/actionitems"),
            "pay_url": format!("{base}/pay"),
        })
        .to_string(),
    )
    .unwrap();
    dir
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Server {
    fn start(port: u16) -> Self {
        Self::start_in(config_home(port))
    }

    fn start_in(config_home: PathBuf) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cpmfl"))
            .arg("mcp")
            .env("XDG_CONFIG_HOME", config_home)
            .env("CPMFL_TOKEN", TOKEN)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("cpmfl starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut server = Self {
            child,
            stdin,
            lines,
            next_id: 1,
        };
        server.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"}
            }),
        );
        server
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(30))
                .expect("a response within 30 s");
            let msg: Value = serde_json::from_str(&line).unwrap();
            if msg["id"] == json!(id) {
                return msg;
            }
        }
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))["result"].clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn every_tool_call() -> Vec<(&'static str, Value)> {
    vec![
        ("campbell_balance", json!({})),
        ("campbell_balance", json!({"account": "SMP100000"})),
        ("campbell_bills", json!({})),
        ("campbell_payments", json!({"since": "2026-01-01"})),
        ("campbell_requests", json!({"include_closed": true})),
        ("campbell_documents", json!({})),
        ("campbell_documents", json!({"folder": 1050})),
        ("campbell_documents", json!({"query": "statement"})),
    ]
}

#[test]
fn every_tool_answers_with_gets_to_the_read_endpoints_alone() {
    let (stub, port) = Stub::start(answer);
    let mut server = Server::start(port);

    let listed = server.request("tools/list", json!({}));
    let mut called: Vec<String> = Vec::new();
    for tool in listed["result"]["tools"].as_array().unwrap() {
        called.push(tool["name"].as_str().unwrap().to_string());
    }
    for (name, _) in every_tool_call() {
        called.retain(|c| c != name);
    }
    assert!(called.is_empty(), "no call below reaches {called:?}");

    for (name, args) in every_tool_call() {
        let result = server.call(name, args.clone());
        assert_eq!(result["isError"], false, "{name} {args}: {result}");
    }

    let seen = stub.seen();
    assert!(!seen.is_empty());
    for (request, authorization) in &seen {
        assert!(
            READS.contains(&request.as_str()),
            "a tool sent {request}, which is not one of the reads"
        );
        assert_eq!(authorization, &format!("Bearer {TOKEN}"), "{request}");
    }
    for read in READS {
        assert!(
            seen.iter().any(|(request, _)| request == read),
            "nothing reached {read}"
        );
    }
}

/// A lapsed session is reported, and the server does not sign in again: no POST,
/// and so no password, however many tools are called.
#[test]
fn a_rejected_session_is_an_auth_error_and_nothing_is_sent_but_the_read() {
    let (stub, port) = Stub::start(|_, _| (401, "{}".into()));
    let mut server = Server::start(port);
    for (name, args) in every_tool_call() {
        let result = server.call(name, args);
        assert_eq!(result["isError"], true, "{name}: {result}");
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("auth:"), "{name}: {text}");
    }
    for (request, _) in stub.seen() {
        assert!(request.starts_with("GET "), "{request}");
        assert!(!request.contains("Authenticate"), "{request}");
    }
}

/// The service URLs come from the config, and the token goes to them.
#[test]
fn a_tool_refuses_a_service_url_off_https() {
    let home = config_home(1);
    std::fs::write(
        home.join("cpmfl/config.json"),
        json!({"pay_url": "http://portal.example/pay"}).to_string(),
    )
    .unwrap();
    let mut server = Server::start_in(home);
    let result = server.call("campbell_balance", json!({}));
    assert_eq!(result["isError"], true, "{result}");
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("other:"), "{text}");
    assert!(text.contains("only https"), "{text}");
}

/// A fixture with one top-level field gone or set, as a changed API would answer.
fn altered(name: &str, field: &str, value: Option<Value>) -> String {
    let mut body: Value = serde_json::from_str(&fixture(name)).unwrap();
    let object = body.as_object_mut().unwrap();
    match value {
        Some(value) => object.insert(field.into(), value),
        None => object.remove(field),
    };
    body.to_string()
}

fn assert_upstream(port: u16, tools: &[(&str, Value)]) {
    let mut server = Server::start(port);
    for (name, args) in tools {
        let result = server.call(name, args.clone());
        assert_eq!(result["isError"], true, "{name}: {result}");
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("upstream:"), "{name}: {text}");
    }
}

/// The tools that page through a `member` list.
fn paged_tools() -> [(&'static str, Value); 3] {
    [
        ("campbell_bills", json!({})),
        ("campbell_payments", json!({})),
        ("campbell_requests", json!({})),
    ]
}

fn paged_fixture(path: &str) -> Option<&'static str> {
    match path {
        "/pay/Ledger" => Some("ledger.json"),
        "/pay/Payment" => Some("payments.json"),
        "/actionitems/HomeownerRequests" => Some("homeowner_requests.json"),
        _ => None,
    }
}

#[test]
fn a_list_with_no_rows_field_is_an_error_not_an_empty_list() {
    let (_stub, port) = Stub::start(|path, query| match paged_fixture(path) {
        Some(_) => (200, "{}".into()),
        None => answer(path, query),
    });
    assert_upstream(port, &paged_tools());
}

/// A page count gone would read as "this is the last page", so the first page
/// would pass for the whole list.
#[test]
fn a_list_with_no_page_count_is_an_error_not_a_short_list() {
    let (_stub, port) = Stub::start(|path, query| match paged_fixture(path) {
        Some(name) => (200, altered(name, "lastPage", None)),
        None => answer(path, query),
    });
    assert_upstream(port, &paged_tools());

    let (_stub, port) = Stub::start(|path, query| {
        if path.ends_with("/Documents/Search") {
            return (200, altered("document_search.json", "totalPages", None));
        }
        answer(path, query)
    });
    assert_upstream(
        port,
        &[("campbell_documents", json!({"query": "statement"}))],
    );
}

/// The portal pages by number, so a row arriving during a walk puts the last row of one page first
/// on the next.
#[test]
fn a_row_repeated_on_the_next_page_is_kept_once() {
    let (_stub, port) = Stub::start(|path, query| {
        if path == "/pay/Payment" {
            let body: Value = serde_json::from_str(&fixture("payments.json")).unwrap();
            let rows = body["member"].as_array().unwrap();
            let page = if query.contains("page=1") {
                &rows[..2]
            } else {
                &rows[1..3]
            };
            let body = json!({"page": 1, "lastPage": 2, "totalItems": 3, "member": page});
            return (200, body.to_string());
        }
        answer(path, query)
    });
    let mut server = Server::start(port);
    let result = server.call("campbell_payments", json!({"response_format": "detailed"}));
    assert_eq!(result["isError"], false, "{result}");
    let page: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 3, "{page}");
}

/// Past the most pages one call reads, the answer is incomplete and says so.
#[test]
fn a_list_past_the_page_limit_is_an_error_not_a_short_list() {
    let (stub, port) = Stub::start(|path, query| {
        if path == "/pay/Ledger" {
            return (200, altered("ledger.json", "lastPage", Some(json!(1000))));
        }
        answer(path, query)
    });
    assert_upstream(port, &[("campbell_bills", json!({}))]);
    let reads = stub
        .seen()
        .iter()
        .filter(|(request, _)| request == "GET /pay/Ledger")
        .count();
    assert_eq!(reads, 100);
}
