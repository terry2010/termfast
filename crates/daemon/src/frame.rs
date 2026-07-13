//! Length-prefixed frame encoding/decoding for IPC — FP-1.7
//!
//! Format: [4-byte length (big-endian u32)][JSON payload]

use anyhow::{Result, bail};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum frame size (1 MB)
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;

/// Write a length-prefixed frame
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, data: &[u8]) -> Result<()> {
    if data.len() > MAX_FRAME_SIZE {
        bail!("frame too large: {} bytes (max {})", data.len(), MAX_FRAME_SIZE);
    }
    let len = data.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}

/// Read a length-prefixed frame
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > MAX_FRAME_SIZE {
        bail!("frame too large: {} bytes (max {})", len, MAX_FRAME_SIZE);
    }

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Write a JSON value as a frame
pub async fn write_json<W: AsyncWrite + Unpin, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<()> {
    let data = serde_json::to_vec(value)?;
    write_frame(writer, &data).await
}

/// Read a frame and deserialize as JSON
pub async fn read_json<R: AsyncRead + Unpin, T: serde::de::DeserializeOwned>(
    reader: &mut R,
) -> Result<T> {
    let data = read_frame(reader).await?;
    let value = serde_json::from_slice(&data)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_frame_round_trip() {
        let mut buf = Vec::new();
        let data = b"hello world";
        write_frame(&mut buf, data).await.unwrap();

        let mut reader = &buf[..];
        let result = read_frame(&mut reader).await.unwrap();
        assert_eq!(result, data);
    }

    #[tokio::test]
    async fn test_empty_frame() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"").await.unwrap();

        let mut reader = &buf[..];
        let result = read_frame(&mut reader).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_large_frame() {
        let data = vec![0xABu8; 100_000];
        let mut buf = Vec::new();
        write_frame(&mut buf, &data).await.unwrap();

        let mut reader = &buf[..];
        let result = read_frame(&mut reader).await.unwrap();
        assert_eq!(result.len(), 100_000);
    }

    #[tokio::test]
    async fn test_json_round_trip() {
        let mut buf = Vec::new();
        let value = serde_json::json!({"key": "value", "num": 42});
        write_json(&mut buf, &value).await.unwrap();

        let mut reader = &buf[..];
        let result: serde_json::Value = read_json(&mut reader).await.unwrap();
        assert_eq!(result, value);
    }

    #[tokio::test]
    async fn test_multiple_frames() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"first").await.unwrap();
        write_frame(&mut buf, b"second").await.unwrap();
        write_frame(&mut buf, b"third").await.unwrap();

        let mut reader = &buf[..];
        assert_eq!(read_frame(&mut reader).await.unwrap(), b"first");
        assert_eq!(read_frame(&mut reader).await.unwrap(), b"second");
        assert_eq!(read_frame(&mut reader).await.unwrap(), b"third");
    }
}
