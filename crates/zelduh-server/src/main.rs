//! The Zelduh signalling server.
//!
//! Multiplayer is peer to peer: players connect directly to each other over
//! WebRTC and the game never passes through a server at all. What peers do
//! need is somewhere to swap the handful of messages that set a direct
//! connection up, and by default that is public infrastructure, which is why
//! the game works from static hosting with nothing running here.
//!
//! This is for when that will not do: a network with no route to the
//! internet, or a preference for keeping your room codes to yourself. Run it
//! and put its address in the page's relay box.
//!
//! It also serves the page, so one process is the whole thing:
//!
//! ```text
//! zelduh-server --port 8080 --dir web
//! ```
//!
//! No dependencies beyond the standard library: the WebSocket handshake and
//! framing are a couple of hundred lines in `ws`, the pub/sub is in `relay`,
//! and static file serving is a few dozen lines below.

mod json;
mod relay;
mod ws;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use relay::Relay;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut port = 8080u16;
    let mut dir = PathBuf::from("web");

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                i += 1;
                port = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(port);
            }
            "--dir" | "-d" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    dir = PathBuf::from(v);
                }
            }
            "--help" | "-h" => {
                println!(
                    "zelduh-server [--port N] [--dir PATH]\n\n\
                     Serves the page and introduces players to each other.\n\
                     The game itself runs peer to peer and never comes through here."
                );
                return;
            }
            other => eprintln!("warning: ignoring {other}"),
        }
        i += 1;
    }

    let relay = Arc::new(Mutex::new(Relay::new()));

    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("could not listen on port {port}: {e}");
            std::process::exit(1);
        }
    };
    println!("zelduh-server listening on http://localhost:{port}");
    println!("  serving {}", dir.display());
    println!("  signalling at ws://localhost:{port}/ws — put that in the page's relay box");

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let relay = Arc::clone(&relay);
        let dir = dir.clone();
        thread::spawn(move || {
            if let Err(e) = serve(stream, relay, dir) {
                // A browser closing a tab shows up as a broken pipe; it is not
                // worth a line of log each time.
                if e.kind() != std::io::ErrorKind::BrokenPipe {
                    eprintln!("connection ended: {e}");
                }
            }
        });
    }
}

/// Reads one HTTP request and either upgrades it or serves a file.
fn serve(stream: TcpStream, relay: Arc<Mutex<Relay>>, dir: PathBuf) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let wants_upgrade = headers
        .get("upgrade")
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);

    if wants_upgrade && path.starts_with("/ws") {
        return websocket(stream, reader, headers, relay);
    }
    if method != "GET" && method != "HEAD" {
        return write_response(stream, "405 Method Not Allowed", "text/plain", b"no");
    }
    serve_file(stream, &dir, &path)
}

/// Completes the handshake, then pumps messages between socket and relay.
fn websocket(
    stream: TcpStream,
    mut reader: BufReader<TcpStream>,
    headers: HashMap<String, String>,
    relay: Arc<Mutex<Relay>>,
) -> std::io::Result<()> {
    let Some(key) = headers.get("sec-websocket-key") else {
        return write_response(stream, "400 Bad Request", "text/plain", b"no key");
    };
    let accept = ws::accept_key(key);
    let mut out = stream.try_clone()?;
    out.write_all(
        format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {accept}\r\n\r\n"
        )
        .as_bytes(),
    )?;
    out.flush()?;

    let (tx, rx): (Sender<String>, Receiver<String>) = channel();
    let id = {
        let mut r = lock(&relay);
        let id = r.connect(tx);
        println!(
            "peer connected ({} here, {} rooms)",
            r.client_count(),
            r.topic_count()
        );
        id
    };

    // One thread does all the writing, so a slow socket never holds up the
    // relay lock and the peers waiting behind it.
    let mut writer = stream.try_clone()?;
    let writer_thread = thread::spawn(move || {
        while let Ok(msg) = rx.recv() {
            if ws::write_frame(&mut writer, ws::Opcode::Text, msg.as_bytes()).is_err() {
                break;
            }
        }
        let _ = ws::write_frame(&mut writer, ws::Opcode::Close, &[]);
    });

    let result = read_loop(&mut reader, &relay, id);
    {
        let mut r = lock(&relay);
        r.disconnect(id);
        println!(
            "peer left ({} here, {} rooms)",
            r.client_count(),
            r.topic_count()
        );
    }
    let _ = writer_thread.join();
    result
}

/// Handles messages from one client until the socket closes.
fn read_loop(
    reader: &mut BufReader<TcpStream>,
    relay: &Arc<Mutex<Relay>>,
    id: usize,
) -> std::io::Result<()> {
    loop {
        let Some(frame) = ws::read_frame(reader)? else {
            return Ok(());
        };
        match frame.opcode {
            ws::Opcode::Close => return Ok(()),
            ws::Opcode::Ping | ws::Opcode::Pong => continue,
            // Signalling is JSON text. A binary frame is not this protocol.
            ws::Opcode::Binary => continue,
            ws::Opcode::Text => {}
            _ => continue,
        }
        let Ok(text) = String::from_utf8(frame.payload) else {
            continue;
        };
        lock(relay).handle(id, &text);
    }
}

/// Takes the relay lock, recovering rather than panicking if a thread died
/// while holding it.
fn lock(relay: &Arc<Mutex<Relay>>) -> std::sync::MutexGuard<'_, Relay> {
    match relay.lock() {
        Ok(r) => r,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ----- static files ------------------------------------------------------

fn serve_file(stream: TcpStream, dir: &Path, path: &str) -> std::io::Result<()> {
    let clean = path.split('?').next().unwrap_or("/");
    let clean = if clean == "/" { "/index.html" } else { clean };

    // Refuse anything that tries to climb out of the served directory.
    if clean.contains("..") || clean.contains('\\') {
        return write_response(stream, "403 Forbidden", "text/plain", b"no");
    }
    let file = dir.join(clean.trim_start_matches('/'));
    match std::fs::read(&file) {
        Ok(body) => write_response(stream, "200 OK", content_type(&file), &body),
        Err(_) => write_response(stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("txt") | Some("zprofile") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn write_response(
    mut stream: TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-cache\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types_cover_what_the_page_needs() {
        assert_eq!(
            content_type(Path::new("a/index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("zelduh.wasm")), "application/wasm");
        assert_eq!(
            content_type(Path::new("main.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("mystery.bin")),
            "application/octet-stream"
        );
    }
}
