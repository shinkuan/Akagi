//! Minimal HTTP mock server for the inference-API / purchase clients' tests.
//!
//! Shared by [`crate::bot::api`], [`crate::bot::purchase`] and
//! [`crate::bot::native`], which all need to assert on the *raw* request the
//! client put on the wire (path, headers, body) and to script the response.
//!
//! One canned response per accepted connection; every response carries
//! `Connection: close` so the client opens a fresh connection for the next
//! request and the responses stay in order.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;
use std::time::Duration;

/// Serve one canned `(status_line, json_body)` per connection, capture each raw
/// request, and return them all when the last response has been written.
///
/// Returns `(base_url, handle)`. Join the handle **after** the client has made
/// its requests to get them back: the thread blocks in `accept()` until it has
/// served all `responses`, so joining early deadlocks the test. Script exactly
/// as many responses as the client will request — one fewer is also how you
/// simulate "the server went away mid-conversation": the listener drops after
/// the last scripted response, and the next connect is refused.
pub fn mock_http(responses: Vec<(&'static str, String)>) -> (String, JoinHandle<Vec<String>>) {
    mock_http_with_delays(
        responses
            .into_iter()
            .map(|(status, body)| (status, body, Duration::ZERO))
            .collect(),
    )
}

/// Variant of [`mock_http`] that waits before each scripted response. Tests use
/// it to prove caller-supplied request budgets are actually enforced.
pub fn mock_http_with_delays(
    responses: Vec<(&'static str, String, Duration)>,
) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for (status_line, body, delay) in responses {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                let n = sock.read(&mut tmp).unwrap();
                assert!(n > 0, "client hung up mid-request");
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                    let want: usize = head
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse().ok())?
                        })
                        .unwrap_or(0);
                    while buf.len() - (pos + 4) < want {
                        let n = sock.read(&mut tmp).unwrap();
                        assert!(n > 0, "client hung up mid-body");
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    break;
                }
            }
            seen.push(String::from_utf8_lossy(&buf).into_owned());
            std::thread::sleep(delay);
            let resp = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            );
            let _ = sock.write_all(resp.as_bytes());
        }
        seen
    });
    (format!("http://{addr}"), handle)
}

/// A `base_url` pointing at a closed port: connecting to it fails immediately
/// (ECONNREFUSED) rather than hanging, which is what a "server is down" test
/// wants. Port 1 (tcpmux) is never bound on developer or CI machines.
pub const UNREACHABLE_BASE_URL: &str = "http://127.0.0.1:1";
