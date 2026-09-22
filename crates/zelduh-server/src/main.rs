//! The Zelduh multiplayer server.
//!
//! One process serves the web page and runs the shared game. It has no
//! dependencies beyond the engine itself: the WebSocket handshake and framing
//! are a couple of hundred lines in `ws`, and the static file serving is a few
//! dozen more, which is less code than vendoring a web framework would add.
//!
//! ```text
//! zelduh-server --port 8080 --dir web --seed 1234
//! ```

mod session;
mod ws;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use session::{c2s, Role, Session, TICK_HZ};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut port = 8080u16;
    let mut dir = PathBuf::from("web");
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1);

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
            "--seed" | "-s" => {
                i += 1;
                seed = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(seed);
            }
            "--help" | "-h" => {
                println!(
                    "zelduh-server [--port N] [--dir PATH] [--seed N]\n\n\
                     Serves the web page and runs a shared world. Up to {} players.",
                    session::MAX_PLAYERS
                );
                return;
            }
            other => eprintln!("warning: ignoring {other}"),
        }
        i += 1;
    }

    let session = Arc::new(Mutex::new(Session::new(seed)));
    spawn_tick_loop(Arc::clone(&session));

    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("could not listen on port {port}: {e}");
            std::process::exit(1);
        }
    };
    println!("zelduh-server listening on http://localhost:{port}");
    println!(
        "  world seed {seed}, up to {} players",
        session::MAX_PLAYERS
    );
    println!("  serving {}", dir.display());

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let session = Arc::clone(&session);
        let dir = dir.clone();
        thread::spawn(move || {
            if let Err(e) = serve(stream, session, dir) {
                // A browser closing a tab shows up as a broken pipe; it is not
                // worth a line of log each time.
                if e.kind() != std::io::ErrorKind::BrokenPipe {
                    eprintln!("connection ended: {e}");
                }
            }
        });
    }
}

/// Steps the world at a fixed rate and tells everyone what happened.
fn spawn_tick_loop(session: Arc<Mutex<Session>>) {
    thread::spawn(move || {
        let period = Duration::from_nanos(1_000_000_000 / TICK_HZ);
        let start = Instant::now();
        let mut ticks = 0u64;
        loop {
            ticks += 1;
            // Sleep to the next absolute deadline rather than for a fixed
            // period, so the clock does not drift over a long session.
            let deadline = start + period * ticks as u32;
            let now = Instant::now();
            if deadline > now {
                thread::sleep(deadline - now);
            }
            let mut s = match session.lock() {
                Ok(s) => s,
                Err(poisoned) => poisoned.into_inner(),
            };
            if s.is_empty() {
                continue;
            }
            let msg = s.tick();
            s.broadcast(&msg);
        }
    });
}

/// Reads one HTTP request and either upgrades it or serves a file.
fn serve(stream: TcpStream, session: Arc<Mutex<Session>>, dir: PathBuf) -> std::io::Result<()> {
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
        return websocket(stream, reader, headers, &path, session);
    }
    if method != "GET" && method != "HEAD" {
        return write_response(stream, "405 Method Not Allowed", "text/plain", b"no");
    }
    serve_file(stream, &dir, &path)
}

/// Completes the handshake, then relays between the socket and the session.
fn websocket(
    stream: TcpStream,
    mut reader: BufReader<TcpStream>,
    headers: HashMap<String, String>,
    path: &str,
    session: Arc<Mutex<Session>>,
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

    // ?role=boss asks to play as the monster.
    let role = if path.contains("role=boss") {
        Role::Boss
    } else {
        Role::Hero
    };

    let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = channel();
    let joined = {
        let mut s = lock(&session);
        s.join(tx, role)
    };
    let Some((slot, welcome)) = joined else {
        let _ = ws::write_frame(
            &mut out,
            ws::Opcode::Binary,
            &session::info("this game is full"),
        );
        let _ = ws::write_frame(&mut out, ws::Opcode::Close, &[]);
        return Ok(());
    };
    {
        let s = lock(&session);
        println!(
            "player {slot} joined as {role:?} ({} playing)",
            s.player_count()
        );
    }

    // One thread does all the writing, so the game loop never blocks on a
    // slow socket.
    let mut writer = stream.try_clone()?;
    let writer_thread = thread::spawn(move || {
        while let Ok(msg) = rx.recv() {
            if ws::write_frame(&mut writer, ws::Opcode::Binary, &msg).is_err() {
                break;
            }
        }
        let _ = ws::write_frame(&mut writer, ws::Opcode::Close, &[]);
    });

    {
        let s = lock(&session);
        s.send_to(slot, &welcome);
    }

    let result = read_loop(&mut reader, &session, slot);

    {
        let mut s = lock(&session);
        s.leave(slot);
    }
    println!("player {slot} left");
    let _ = writer_thread.join();
    result
}

/// Handles messages from one client until the socket closes.
fn read_loop(
    reader: &mut BufReader<TcpStream>,
    session: &Arc<Mutex<Session>>,
    slot: usize,
) -> std::io::Result<()> {
    loop {
        let Some(frame) = ws::read_frame(reader)? else {
            return Ok(());
        };
        match frame.opcode {
            ws::Opcode::Close => return Ok(()),
            ws::Opcode::Ping | ws::Opcode::Pong | ws::Opcode::Text => continue,
            _ => {}
        }
        let payload = frame.payload;
        if payload.is_empty() {
            continue;
        }
        match payload[0] {
            c2s::INPUT if payload.len() >= 3 => {
                let buttons = u16::from_le_bytes([payload[1], payload[2]]);
                lock(session).set_input(slot, buttons);
            }
            c2s::CHECKSUM if payload.len() >= 13 => {
                let frame_no = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
                let sum = u64::from_le_bytes([
                    payload[5],
                    payload[6],
                    payload[7],
                    payload[8],
                    payload[9],
                    payload[10],
                    payload[11],
                    payload[12],
                ]);
                let mut s = lock(session);
                if let Some(reply) = s.check(frame_no, sum) {
                    eprintln!("player {slot} has drifted out of step at frame {frame_no}");
                    s.send_to(slot, &reply);
                }
            }
            c2s::ROLE if payload.len() >= 2 => {
                if payload[1] == 1 {
                    lock(session).request_boss(slot);
                }
            }
            _ => {}
        }
    }
}

/// Takes the session lock, recovering rather than panicking if a thread died
/// while holding it.
fn lock(session: &Arc<Mutex<Session>>) -> std::sync::MutexGuard<'_, Session> {
    match session.lock() {
        Ok(s) => s,
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
