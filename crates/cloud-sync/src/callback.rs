//! Local OAuth callback server — listens on localhost for OAuth redirects.
//!
//! When the user authorizes in the browser, the provider redirects to
//! `http://localhost:PORT/callback?code=xxx&state=xxx`. This server
//! captures the code and state, then shuts down.

use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Result of waiting for the OAuth callback.
pub struct CallbackResult {
    pub code: String,
    pub state: String,
    /// Full query string (for providers that return extra params)
    pub query: String,
}

/// A local OAuth callback server.
pub struct CallbackServer {
    pub port: u16,
    rx: Arc<Mutex<Option<oneshot::Receiver<CallbackResult>>>>,
}

impl CallbackServer {
    /// Start a callback server on the given port.
    /// Returns the server handle; call `wait_for_callback` to block until
    /// the browser redirects back.
    pub async fn start(port: u16) -> Result<Self, std::io::Error> {
        let (tx, rx) = oneshot::channel::<CallbackResult>();
        let tx = Arc::new(Mutex::new(Some(tx)));
        let rx = Arc::new(Mutex::new(Some(rx)));

        let addr = format!("127.0.0.1:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 4096];
                let mut stream = stream;
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);

                // Parse the request line: GET /callback?code=xxx&state=yyy HTTP/1.1
                let first_line = request.lines().next().unwrap_or("");
                let path = first_line.split_whitespace().nth(1).unwrap_or("");

                // Extract query string
                let query = path.split('?').nth(1).unwrap_or("");

                // Parse code and state from query
                let mut code = String::new();
                let mut state = String::new();
                for pair in query.split('&') {
                    let mut kv = pair.splitn(2, '=');
                    let key = kv.next().unwrap_or("");
                    let val = kv.next().unwrap_or("");
                    let decoded = urlencoding::decode(val)
                        .map(|s| s.into_owned())
                        .unwrap_or_else(|_| val.to_string());
                    match key {
                        "code" => code = decoded,
                        "state" => state = decoded,
                        _ => {}
                    }
                }

                // Send a response to the browser
                let body = if code.is_empty() {
                    "<html><body><h2>Authorization failed</h2><p>No code received. You can close this tab.</p></body></html>"
                } else {
                    "<html><body><h2>Authorization successful</h2><p>You can close this tab and return to TermFast.</p></body></html>"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;

                // Send result to the channel
                if let Some(tx) = tx_clone.lock().unwrap().take() {
                    let _ = tx.send(CallbackResult {
                        code,
                        state,
                        query: query.to_string(),
                    });
                }
            }
        });

        Ok(Self { port, rx })
    }

    /// Wait for the OAuth callback. Returns the code and state.
    /// Times out after 5 minutes.
    pub async fn wait_for_callback(self) -> Result<CallbackResult, crate::CloudSyncError> {
        let rx = self
            .rx
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| crate::CloudSyncError::OAuth("callback channel already consumed".into()))?;

        match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => Err(crate::CloudSyncError::OAuth("callback sender dropped".into())),
            Err(_) => Err(crate::CloudSyncError::OAuth("callback timed out (5 min)".into())),
        }
    }

    /// Consume the receiver without waiting — for splitting start/wait into two calls.
    pub fn wait_for_callback_consumer(
        self,
    ) -> tokio::sync::oneshot::Receiver<CallbackResult> {
        self.rx
            .lock()
            .unwrap()
            .take()
            .expect("callback channel already consumed")
    }

    /// Get the redirect_uri to use for this server.
    pub fn redirect_uri(&self) -> String {
        format!("http://localhost:{}/callback", self.port)
    }
}

// === SECTION callback END ===
