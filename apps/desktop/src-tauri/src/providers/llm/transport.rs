//! Bounded transport buffers shared by both SSE wire formats.
use super::LlmError;

pub(super) const MAX_EVENT_BYTES: usize = 128 * 1024;
const MAX_ERROR_BYTES: usize = 16 * 1024;

#[derive(Default)]
pub(super) struct EventBuffer(Vec<u8>);

impl EventBuffer {
    pub(super) fn push(
        &mut self,
        byte: u8,
        provider: &'static str,
    ) -> Result<Option<String>, LlmError> {
        if self.0.len() == MAX_EVENT_BYTES {
            return Err(oversized(provider));
        }
        self.0.push(byte);
        if self.0.ends_with(b"\n\n") || self.0.ends_with(b"\r\n\r\n") {
            return self.take(provider).map(Some);
        }
        Ok(None)
    }

    pub(super) fn take(&mut self, provider: &'static str) -> Result<String, LlmError> {
        // Decode only complete events, so UTF-8 split across network chunks is valid.
        String::from_utf8(std::mem::take(&mut self.0)).map_err(|_| LlmError::InvalidResponse {
            provider,
            message: "Invalid UTF-8 in provider stream".into(),
        })
    }
}

pub(super) async fn error_body(mut response: reqwest::Response) -> String {
    let mut body = Vec::new();
    // The HTTP error status is authoritative even if its optional diagnostic
    // body is interrupted. Return only the bounded bytes received so far.
    while let Ok(Some(bytes)) = response.chunk().await {
        let count = bytes.len().min(MAX_ERROR_BYTES - body.len());
        body.extend_from_slice(&bytes[..count]);
        if body.len() == MAX_ERROR_BYTES {
            // Error status remains authoritative even when its body is huge.
            // Stop reading immediately; only the bounded prefix is classified.
            break;
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

fn oversized(provider: &'static str) -> LlmError {
    LlmError::InvalidResponse {
        provider,
        message: "Provider transport buffer limit exceeded".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_buffer_before_growth_and_decodes_utf8_after_framing() {
        let mut buffer = EventBuffer::default();
        for _ in 0..MAX_EVENT_BYTES {
            assert!(buffer.push(b'x', "test").unwrap().is_none());
        }
        assert!(buffer.push(b'x', "test").is_err());
        assert_eq!(buffer.0.len(), MAX_EVENT_BYTES);
        for delimiter in ["\n\n", "\r\n\r\n"] {
            let mut buffer = EventBuffer::default();
            let event = format!("data: café{delimiter}");
            let mut events = Vec::new();
            for byte in event.bytes() {
                events.extend(buffer.push(byte, "test").unwrap());
            }
            assert_eq!(events, [event]);
            assert_eq!(buffer.take("test").unwrap(), "");
        }
    }

    #[tokio::test]
    async fn interrupted_error_body_keeps_received_prefix_and_http_status() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            {
                let mut reader = tokio::io::BufReader::new(&mut socket);
                let mut headers = String::new();
                while !headers.ends_with("\r\n\r\n") {
                    assert!(reader.read_line(&mut headers).await.unwrap() > 0);
                }
            }
            // Send real partial bytes, then close before Content-Length is met.
            socket.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 100\r\nConnection: close\r\n\r\npartial diagnostic").await.unwrap();
            socket.shutdown().await.unwrap();
        });
        let response = reqwest::Client::new()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let prefix = error_body(response).await;
        assert_eq!(prefix, "partial diagnostic");
        let error = super::super::openai_compat::map_http_error("test", status, &headers, &prefix);
        assert_eq!(error.code(), "LLM_AUTH_FAILED");
        server.await.unwrap();
    }
}
