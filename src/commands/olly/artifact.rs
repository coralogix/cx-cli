use std::io::Read;

use flate2::read::GzDecoder;

use crate::error::{CxError, Result};

/// Result of downloading artifact content.
#[derive(Debug)]
pub enum ArtifactContent {
    /// Text content that can be displayed in terminal.
    Text(String),
    Binary,
}

/// Download content from a presigned URL.
/// Decompresses gzip content.
pub async fn download_content(url: &str) -> Result<ArtifactContent> {
    let response = reqwest::get(url).await?;

    if !response.status().is_success() {
        return Err(CxError::Api {
            status: response.status().as_u16(),
            message: "Failed to download artifact content".to_string(),
        });
    }

    let bytes = response.bytes().await?;
    let data = process_content(bytes.to_vec());
    Ok(data)
}

/// Process downloaded bytes: decompress gzip.
fn process_content(bytes: Vec<u8>) -> ArtifactContent {
    // Decompress gzip content
    let data = decompress_gzip(&bytes).unwrap_or(bytes);

    // Try to interpret as UTF-8 text
    match String::from_utf8(data) {
        Ok(text) => ArtifactContent::Text(text),
        Err(_) => ArtifactContent::Binary,
    }
}

/// Decompress gzip data.
fn decompress_gzip(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_content_text() {
        let text = "Hello, world!";
        let result = process_content(text.as_bytes().to_vec());
        match result {
            ArtifactContent::Text(s) => assert_eq!(s, text),
            ArtifactContent::Binary => panic!("Expected Text, got Binary"),
        }
    }

    #[test]
    fn process_content_json() {
        let json = r#"{"key": "value", "number": 42}"#;
        let result = process_content(json.as_bytes().to_vec());
        match result {
            ArtifactContent::Text(s) => assert_eq!(s, json),
            ArtifactContent::Binary => panic!("Expected Text, got Binary"),
        }
    }

    #[test]
    fn process_content_binary() {
        // Invalid UTF-8 sequence
        let bytes = vec![0xff, 0xfe, 0x00, 0x01];
        let result = process_content(bytes);
        assert!(matches!(result, ArtifactContent::Binary));
    }

    #[test]
    fn process_content_gzip_text() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let original = "Hello, compressed world!";

        // Compress the text
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();

        // Verify it has gzip magic bytes
        assert_eq!(compressed[0], 0x1f);
        assert_eq!(compressed[1], 0x8b);

        // Process should decompress and return text
        let result = process_content(compressed);
        match result {
            ArtifactContent::Text(s) => assert_eq!(s, original),
            ArtifactContent::Binary => panic!("Expected Text after gzip decompression"),
        }
    }

    #[test]
    fn process_content_gzip_binary() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        // Binary data that's not valid UTF-8
        let original = vec![0xff, 0xfe, 0x00, 0x01, 0x80, 0x90];

        // Compress the binary
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&original).unwrap();
        let compressed = encoder.finish().unwrap();

        // Process should decompress but return binary (not valid UTF-8)
        let result = process_content(compressed);
        assert!(matches!(result, ArtifactContent::Binary));
    }
}
