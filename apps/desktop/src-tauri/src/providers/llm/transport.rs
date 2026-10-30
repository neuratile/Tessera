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

pub(super) async fn error_body(
    mut response: reqwest::Response,
    provider: &'static str,
) -> Result<String, LlmError> {
    let mut body = Vec::new();
    while let Some(bytes) = response
        .chunk()
        .await
        .map_err(|_| LlmError::StreamInterrupted {
            provider,
            message: "Failed reading provider error response".into(),
        })?
    {
        let count = bytes.len().min(MAX_ERROR_BYTES - body.len());
        body.extend_from_slice(&bytes[..count]);
        if body.len() == MAX_ERROR_BYTES {
            // Error status remains authoritative even when its body is huge.
            // Stop reading immediately; only the bounded prefix is classified.
            break;
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
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
}
