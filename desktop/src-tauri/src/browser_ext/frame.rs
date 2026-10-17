//! Native messaging framing: each message is UTF-8 JSON preceded by its length as a 32-bit
//! unsigned integer in the machine's byte order. The browser uses it on stdin/stdout; the host
//! and the app use the same framing on the local socket, so the host can relay frames as is.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt};

/// Our messages are a few hundred bytes. Chrome caps host-to-browser messages at 1 MB; a larger
/// length on any side means a broken or hostile peer, so the connection is dropped instead of
/// allocating it.
pub const MAX_FRAME: usize = 1024 * 1024;

/// The length prefix and the bytes of one message.
pub fn encode(json: &[u8]) -> io::Result<Vec<u8>> {
    if json.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut frame = Vec::with_capacity(4 + json.len());
    frame.extend_from_slice(&(json.len() as u32).to_ne_bytes());
    frame.extend_from_slice(json);
    Ok(frame)
}

/// Read one message body. `Ok(None)` when the peer closed the stream between messages.
pub async fn read<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_ne_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip_of_two_messages_then_end() {
        let mut bytes = encode(br#"{"type":"hello"}"#).unwrap();
        bytes.extend(encode("{\"site\":\"\u{e9}.fr\"}".as_bytes()).unwrap());
        let mut reader = bytes.as_slice();
        assert_eq!(read(&mut reader).await.unwrap().unwrap(), br#"{"type":"hello"}"#);
        // The length counts bytes, not characters ("é" is two bytes).
        assert_eq!(read(&mut reader).await.unwrap().unwrap(), "{\"site\":\"\u{e9}.fr\"}".as_bytes());
        assert_eq!(read(&mut reader).await.unwrap(), None, "a clean end between messages");
    }

    #[tokio::test]
    async fn length_is_in_native_byte_order() {
        let frame = encode(b"{}").unwrap();
        assert_eq!(&frame[..4], &2u32.to_ne_bytes());
    }

    #[tokio::test]
    async fn oversized_and_cut_messages_are_errors() {
        let huge = ((MAX_FRAME + 1) as u32).to_ne_bytes();
        assert!(read(&mut huge.as_slice()).await.is_err(), "no giant allocation from a bad length");
        let mut cut = encode(b"{\"a\":1}").unwrap();
        cut.truncate(6);
        assert!(read(&mut cut.as_slice()).await.is_err(), "a message cut in half is not a clean end");
        assert!(encode(&vec![b' '; MAX_FRAME + 1]).is_err());
    }
}
