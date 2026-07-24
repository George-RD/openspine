//! Loopback HTTP server for receiving OAuth browser redirects.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, thiserror::Error)]
pub enum CallbackError {
    #[error("callback listener I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("callback listener timed out after 3 minutes")]
    Timeout,
    #[error("OAuth state mismatch: expected {expected}, got {got}")]
    StateMismatch { expected: String, got: String },
    #[error("OAuth authorization error returned from provider: {0}")]
    ProviderError(String),
    #[error("missing code query parameter in OAuth callback")]
    MissingCode,
}

pub struct CallbackServer {
    listener: TcpListener,
    port: u16,
}

fn url_decode(input: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        match b {
            b'+' => bytes.push(b' '),
            b'%' => {
                let h1 = chars.next().unwrap_or(b'0');
                let h2 = chars.next().unwrap_or(b'0');
                let hex_bytes = [h1, h2];
                let hex_str = std::str::from_utf8(&hex_bytes).unwrap_or("00");
                if let Ok(byte) = u8::from_str_radix(hex_str, 16) {
                    bytes.push(byte);
                }
            }
            _ => bytes.push(b),
        }
    }
    String::from_utf8_lossy(&bytes).to_string()
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(url_decode(k), url_decode(v));
    }
    map
}

impl CallbackServer {
    pub async fn bind(preferred_port: u16) -> Result<Self, CallbackError> {
        let addr = SocketAddr::from(([127, 0, 0, 1], preferred_port));
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(_) => TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?,
        };
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn wait_for_code(self, expected_state: &str) -> Result<String, CallbackError> {
        let timeout = Duration::from_secs(180);
        tokio::time::timeout(timeout, self.listen_once(expected_state))
            .await
            .map_err(|_| CallbackError::Timeout)?
    }

    async fn listen_once(self, expected_state: &str) -> Result<String, CallbackError> {
        let (mut stream, _) = self.listener.accept().await?;
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await?;
        let request_str = String::from_utf8_lossy(&buf[..n]);

        let mut lines = request_str.lines();
        let request_line = lines.next().unwrap_or_default();
        let path_and_query = request_line.split_whitespace().nth(1).unwrap_or("/");

        let query_str = path_and_query.split_once('?').map(|(_, q)| q).unwrap_or("");
        let query_map = parse_query(query_str);

        if let Some(err) = query_map.get("error") {
            let response = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n<html><body><h2>Authentication Failed</h2><p>Provider returned error.</p></body></html>";
            let _ = stream.write_all(response.as_bytes()).await;
            return Err(CallbackError::ProviderError(err.clone()));
        }

        let state = query_map.get("state").cloned().unwrap_or_default();
        if state != expected_state {
            let response = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n<html><body><h2>Authentication Failed</h2><p>State mismatch.</p></body></html>";
            let _ = stream.write_all(response.as_bytes()).await;
            return Err(CallbackError::StateMismatch {
                expected: expected_state.to_string(),
                got: state,
            });
        }

        let code = query_map
            .get("code")
            .cloned()
            .ok_or(CallbackError::MissingCode)?;

        let html = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n\
            <!DOCTYPE html><html><head><title>OpenSpine OAuth</title></head>\
            <body style='font-family: sans-serif; text-align: center; margin-top: 50px;'>\
            <h2>Authentication Successful!</h2>\
            <p>You can close this window and return to OpenSpine terminal.</p>\
            </body></html>";
        let _ = stream.write_all(html.as_bytes()).await;
        let _ = stream.flush().await;

        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oauth_loopback_callback_server_receives_code_and_validates_state() {
        let server = CallbackServer::bind(0).await.expect("bind server");
        let port = server.port();
        let expected_state = "test-csrf-state-12345";

        let server_handle = tokio::spawn(async move { server.wait_for_code(expected_state).await });

        // Simulate browser callback GET request
        let url = format!(
            "http://127.0.0.1:{port}/callback?code=secret-auth-code-777&state={expected_state}"
        );
        let client = reqwest::Client::new();
        let resp = client.get(&url).send().await.expect("send request");
        assert_eq!(resp.status(), 200);

        let code = server_handle.await.expect("join").expect("code");
        assert_eq!(code, "secret-auth-code-777");
    }
}
