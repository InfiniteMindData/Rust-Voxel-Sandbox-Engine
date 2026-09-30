//! HTTP display backend.
//!
//! The engine has no native window in this environment (no display server and
//! no GPU in the build sandbox), so frames are streamed to a browser instead:
//!
//! ```text
//! main thread (game)                 display thread (per connection)
//! ─────────────────────              ───────────────────────────────
//! simulate -> render frame    ──▶    GET /stream  : multipart PNG stream
//! publish_frame(fb) into slot        GET /frame.png : single frame
//! update_stats()                     GET /stats    : HUD text
//!              ◀──                   POST /input   : keys + mouse deltas
//! ```
//!
//! The browser page (see `assets/ui/index.html`) renders the stream into a
//! `<canvas>`-like `<img>` and posts input back. A native `winit` window
//! backend can implement the same `publish/input` contract later without the
//! game code noticing a difference.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::engine::input::InputState;
use crate::engine::{EngineError, Result};
use crate::log_error;
use crate::log_info;
use crate::log_warn;
use crate::render::framebuffer::Framebuffer;
use crate::render::png;

const LOG_TARGET: &str = "display";
const HTTP_READ_CAP: usize = 32 * 1024;
const INPUT_BODY_CAP: usize = 8 * 1024;
const MAX_CONNECTIONS: u32 = 32;
const MULTIPART_BOUNDARY: &str = "blockscape-frame";

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Live engine statistics surfaced to the debug HUD.
#[derive(Debug, Clone, Copy)]
pub struct EngineStats {
    pub fps: f32,
    pub frame_ms: f32,
    pub width: u32,
    pub height: u32,
    pub uptime_s: f32,
    pub pos: [f32; 3],
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub triangles: u64,
    pub draw_calls: u64,
    pub simulation_hz: f32,
}

impl Default for EngineStats {
    fn default() -> Self {
        Self {
            fps: 0.0,
            frame_ms: 0.0,
            width: 0,
            height: 0,
            uptime_s: 0.0,
            pos: [0.0; 3],
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            triangles: 0,
            draw_calls: 0,
            simulation_hz: 0.0,
        }
    }
}

impl EngineStats {
    /// Formats stats as `key=value` lines for the HUD (parsed client-side
    /// without needing a JSON decoder).
    pub fn to_hud_text(&self) -> String {
        format!(
            "BLOCKSCAPE\n\
             fps         {:>7.1}\n\
             frame       {:>6.1} ms\n\
             sim         {:>6.1} Hz\n\
             resolution  {:>4}x{}\n\
             pos         {:>7.2} {:>7.2} {:>7.2}\n\
             yaw/pitch   {:>6.1} / {:>6.1} deg\n\
             triangles   {:>7}\n\
             draw calls  {:>7}\n\
             uptime      {:>6.1} s\n",
            self.fps,
            self.frame_ms,
            self.simulation_hz,
            self.width,
            self.height,
            self.pos[0],
            self.pos[1],
            self.pos[2],
            self.yaw_deg,
            self.pitch_deg,
            self.triangles,
            self.draw_calls,
            self.uptime_s,
        )
    }
}

struct FrameSlotInner {
    seq: u64,
    width: u32,
    height: u32,
    data: Option<Arc<Vec<u8>>>,
}

struct FrameSlot {
    inner: Mutex<FrameSlotInner>,
    updated: Condvar,
}

impl FrameSlot {
    fn new() -> Self {
        Self {
            inner: Mutex::new(FrameSlotInner {
                seq: 0,
                width: 0,
                height: 0,
                data: None,
            }),
            updated: Condvar::new(),
        }
    }

    fn publish(&self, fb: &Framebuffer) {
        let mut guard = self.inner.lock().expect("frame slot poisoned");
        guard.seq += 1;
        guard.width = fb.width();
        guard.height = fb.height();
        guard.data = Some(Arc::new(fb.color_bytes().to_vec()));
        self.updated.notify_all();
    }

    /// Waits up to `timeout` for a frame newer than `last_seq`.
    fn wait_for_frame(&self, last_seq: u64, timeout: Duration) -> Option<(u64, u32, u32, Arc<Vec<u8>>)> {
        let guard = self.inner.lock().expect("frame slot poisoned");
        let (guard, _tmo) = self
            .updated
            .wait_timeout_while(guard, timeout, |s| s.seq <= last_seq)
            .expect("frame slot poisoned");
        if guard.seq <= last_seq {
            return None;
        }
        guard.data.as_ref().map(|d| (guard.seq, guard.width, guard.height, Arc::clone(d)))
    }

    fn latest(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
        let guard = self.inner.lock().expect("frame slot poisoned");
        guard
            .data
            .as_ref()
            .map(|d| (guard.width, guard.height, Arc::clone(d)))
    }
}

// ---------------------------------------------------------------------------
// Display backend
// ---------------------------------------------------------------------------

/// Display configuration.
#[derive(Debug, Clone)]
pub struct DisplayConfig {
    /// Address to bind. Use `0.0.0.0` to expose the game through a proxy.
    pub bind: SocketAddr,
}

/// Handle to the running display server.
pub struct HttpDisplay {
    frame_slot: Arc<FrameSlot>,
    input: Arc<Mutex<InputState>>,
    stats: Arc<Mutex<EngineStats>>,
    shutdown: Arc<AtomicBool>,
    addr: SocketAddr,
    listener_thread: Option<std::thread::JoinHandle<()>>,
}

impl HttpDisplay {
    /// Binds the listener and spawns the accept loop.
    pub fn start(config: DisplayConfig) -> Result<HttpDisplay> {
        let listener = TcpListener::bind(config.bind).map_err(|e| {
            EngineError::Config(format!("cannot bind display server on {}: {e}", config.bind))
        })?;
        let addr = listener.local_addr()?;

        // Shared state is created up front and cloned into the threads; there
        // is never a second `HttpDisplay` instance whose `Drop` could fire.
        let frame_slot = Arc::new(FrameSlot::new());
        let input = Arc::new(Mutex::new(InputState::new()));
        let stats = Arc::new(Mutex::new(EngineStats::default()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicU32::new(0));

        listener.set_nonblocking(true)?;
        let thread = {
            let shutdown = Arc::clone(&shutdown);
            let frame_slot = Arc::clone(&frame_slot);
            let input = Arc::clone(&input);
            let stats = Arc::clone(&stats);
            let active = Arc::clone(&active);
            std::thread::Builder::new()
                .name("display-accept".into())
                .spawn(move || {
                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                    match listener.accept() {
                        Ok((stream, _peer)) => {
                            if active.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
                                log_warn!(LOG_TARGET, "connection limit reached, dropping client");
                                drop(stream);
                                continue;
                            }
                            active.fetch_add(1, Ordering::Relaxed);
                            let shutdown = Arc::clone(&shutdown);
                            let frame_slot = Arc::clone(&frame_slot);
                            let input = Arc::clone(&input);
                            let stats = Arc::clone(&stats);
                            let active = Arc::clone(&active);
                            std::thread::Builder::new()
                                .name("display-conn".into())
                                .spawn(move || {
                                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                        handle_connection(stream, frame_slot, input, stats, shutdown);
                                    }));
                                    if let Err(panic) = result {
                                        log_error!(LOG_TARGET, "connection handler panicked: {panic:?}");
                                    }
                                    active.fetch_sub(1, Ordering::Relaxed);
                                })
                                .map_err(|e| log_error!(LOG_TARGET, "failed to spawn connection thread: {e}"))
                                .ok();
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        Err(e) => {
                            log_warn!(LOG_TARGET, "accept failed: {e}");
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
                log_info!(LOG_TARGET, "display server stopped");
                })
                .map_err(|e| EngineError::Io(std::io::Error::other(e)))?
        };

        Ok(HttpDisplay {
            frame_slot,
            input,
            stats,
            shutdown,
            addr,
            listener_thread: Some(thread),
        })
    }

    /// The bound address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The shared input state; the simulation locks this briefly each step.
    pub fn input(&self) -> &Arc<Mutex<InputState>> {
        &self.input
    }

    /// Publishes a rendered frame to all stream viewers.
    pub fn publish_frame(&self, fb: &Framebuffer) {
        self.frame_slot.publish(fb);
    }

    /// Updates the HUD statistics snapshot.
    pub fn update_stats(&self, stats: EngineStats) {
        *self.stats.lock().expect("stats poisoned") = stats;
    }
}

impl Drop for HttpDisplay {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(thread) = self.listener_thread.take() {
            let _ = thread.join();
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP plumbing (deliberately tiny)
// ---------------------------------------------------------------------------

struct Request {
    method: String,
    path: String,
    content_length: usize,
}

/// Reads one CRLF-terminated line (without the CRLF), capped by `cap`.
fn read_line(stream: &mut TcpStream, cap: usize) -> std::io::Result<String> {
    let mut line = Vec::with_capacity(64);
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof in header"));
        }
        if byte[0] == b'\n' {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(String::from_utf8_lossy(&line).into_owned());
        }
        line.push(byte[0]);
        if line.len() > cap {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "header line too long"));
        }
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    let request_line = read_line(stream, HTTP_READ_CAP)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing method")
    })?.to_string();
    let path = parts.next().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing path")
    })?.to_string();

    let mut content_length = 0usize;
    loop {
        let line = read_line(stream, HTTP_READ_CAP)?;
        if line.is_empty() {
            break; // end of headers
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    if content_length > INPUT_BODY_CAP {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "request body too large",
        ));
    }

    // Strip query string.
    let path = path.split('?').next().unwrap_or("/").to_string();
    Ok(Request {
        method,
        path,
        content_length,
    })
}

fn read_body(stream: &mut TcpStream, len: usize) -> std::io::Result<Vec<u8>> {
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    Ok(body)
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() + 1 && i + 2 <= bytes.len() - 1 + 1 => {
                if i + 2 < bytes.len() {
                    if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses `a=1&b=2,3` style bodies into `(key, value)` pairs.
fn parse_form(body: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(body);
    text.split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((percent_decode(k), percent_decode(v)))
        })
        .collect()
}

fn apply_input(input: &Arc<Mutex<InputState>>, body: &[u8]) {
    use crate::engine::input::{Key, MouseButton};

    let mut state = match input.lock() {
        Ok(s) => s,
        Err(poisoned) => poisoned.into_inner(),
    };
    for (key, value) in parse_form(body) {
        match key.as_str() {
            // Key events: `ke=KeyW:1,Space:0`
            "ke" => {
                for event in value.split(',') {
                    if let Some((code, flag)) = event.rsplit_once(':') {
                        state.on_key(Key::from_code(code), flag == "1");
                    }
                }
            }
            // Button events: `mb=0:1,2:0`
            "mb" => {
                for event in value.split(',') {
                    if let Some((btn, flag)) = event.rsplit_once(':') {
                        let button = match btn {
                            "0" => MouseButton::Left,
                            "2" => MouseButton::Right,
                            _ => continue,
                        };
                        state.on_button(button, flag == "1");
                    }
                }
            }
            // Accumulated mouse deltas: `mdx=12.5&mdy=-3.0`
            "mdx" => {
                if let Ok(dx) = value.parse::<f64>() {
                    state.on_mouse_delta(dx, 0.0);
                }
            }
            "mdy" => {
                if let Ok(dy) = value.parse::<f64>() {
                    state.on_mouse_delta(0.0, dy);
                }
            }
            _ => {}
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    frame_slot: Arc<FrameSlot>,
    input: Arc<Mutex<InputState>>,
    stats: Arc<Mutex<EngineStats>>,
    shutdown: Arc<AtomicBool>,
) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let request = match read_request(&mut stream) {
            Ok(r) => r,
            Err(_) => break, // client closed, timeout, or malformed request
        };

        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/") => {
                if respond(&mut stream, "200 OK", "text/html; charset=utf-8", crate::engine::window::INDEX_HTML.as_bytes()).is_err() {
                    break;
                }
            }
            ("GET", "/stats") => {
                let text = stats.lock().map(|s| s.to_hud_text()).unwrap_or_default();
                if respond(&mut stream, "200 OK", "text/plain; charset=utf-8", text.as_bytes()).is_err() {
                    break;
                }
            }
            ("GET", "/frame.png") => {
                let frame = frame_slot.latest().map(|(w, h, data)| png::encode_png_rgb(w, h, &data));
                match frame {
                    Some(png_bytes) => {
                        if respond(&mut stream, "200 OK", "image/png", &png_bytes).is_err() {
                            break;
                        }
                    }
                    None => {
                        if respond(&mut stream, "503 Service Unavailable", "text/plain", b"no frame yet").is_err() {
                            break;
                        }
                    }
                }
            }
            ("GET", "/stream") => {
                if stream_multipart(&mut stream, &frame_slot, &shutdown).is_err() {
                    break;
                }
            }
            ("POST", "/input") => {
                let body = match read_body(&mut stream, request.content_length) {
                    Ok(b) => b,
                    Err(_) => break,
                };
                apply_input(&input, &body);
                if respond(&mut stream, "204 No Content", "text/plain", b"").is_err() {
                    break;
                }
            }
            ("GET" | "POST", _) => {
                if respond(&mut stream, "404 Not Found", "text/plain", b"not found").is_err() {
                    break;
                }
            }
            _ => {
                if respond(&mut stream, "405 Method Not Allowed", "text/plain", b"method not allowed").is_err() {
                    break;
                }
            }
        }
    }
}

/// Streams frames as `multipart/x-mixed-replace`, which browsers render
/// natively inside an `<img>` element - no client-side decoding needed.
fn stream_multipart(
    stream: &mut TcpStream,
    frame_slot: &FrameSlot,
    shutdown: &AtomicBool,
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary={MULTIPART_BOUNDARY}\r\nCache-Control: no-store\r\nX-Accel-Buffering: no\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(header.as_bytes())?;

    // Wait for the first frame published *after* this viewer connected
    // (`u64::MAX` can never be reached by the monotonically increasing seq).
    let mut last_seq: u64 = u64::MAX;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let Some((_seq, width, height, data)) = frame_slot.wait_for_frame(last_seq, Duration::from_millis(250)) else {
            continue;
        };
        last_seq = _seq;

        let png = png::encode_png_rgb(width, height, &data);
        let part_header = format!(
            "--{MULTIPART_BOUNDARY}\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\r\n",
            png.len()
        );
        stream.write_all(part_header.as_bytes())?;
        stream.write_all(&png)?;
        stream.write_all(b"\r\n")?;
        stream.flush()?;
    }
    Ok(())
}

/// The browser UI served at `/` (see `assets/ui/index.html`).
pub static INDEX_HTML: &str = include_str!("../../assets/ui/index.html");
