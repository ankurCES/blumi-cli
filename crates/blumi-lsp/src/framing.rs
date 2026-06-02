//! LSP base-protocol framing: `Content-Length` headers + a JSON body.

use serde_json::Value;
use std::io;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

/// Encode a JSON value as a framed LSP message.
pub fn encode(value: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(&body);
    out
}

/// Read one framed message from `r`, or `None` at EOF.
pub async fn read_message<R: AsyncBufReadExt + Unpin>(r: &mut R) -> io::Result<Option<Value>> {
    let mut content_len: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line).await?;
        if n == 0 {
            return Ok(None); // clean EOF before any header
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // blank line terminates the headers
        }
        if let Some(v) = trimmed.strip_prefix("Content-Length:") {
            content_len = v.trim().parse().ok();
        }
    }
    let len = content_len
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    let value =
        serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn frame_roundtrips() {
        let msg = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"x":[1,2,3]}});
        let bytes = encode(&msg);
        assert!(bytes.starts_with(b"Content-Length: "));
        let mut r = BufReader::new(&bytes[..]);
        let back = read_message(&mut r).await.unwrap().unwrap();
        assert_eq!(back, msg);
    }

    #[tokio::test]
    async fn reads_back_to_back_messages_and_eof() {
        let a = json!({"id":1,"result":"a"});
        let b = json!({"id":2,"result":"b"});
        let mut stream = encode(&a);
        stream.extend(encode(&b));
        let mut r = BufReader::new(&stream[..]);
        assert_eq!(read_message(&mut r).await.unwrap().unwrap(), a);
        assert_eq!(read_message(&mut r).await.unwrap().unwrap(), b);
        assert!(read_message(&mut r).await.unwrap().is_none()); // EOF
    }
}
