//! Byte-level vault codec; no production key, filesystem, or policy routing.

use base64::engine::{general_purpose, DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;
use fernet::Fernet;

pub const MAGIC: &[u8] = b"CBV1\n";

#[derive(Debug)]
pub struct VaultError;

fn cipher(key: Option<&str>) -> Result<Fernet, VaultError> {
    key.and_then(Fernet::new).ok_or(VaultError)
}

pub fn seal(key: &str, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let token = cipher(Some(key))?.encrypt(plaintext);
    let mut result = Vec::with_capacity(MAGIC.len() + token.len());
    result.extend_from_slice(MAGIC);
    result.extend_from_slice(token.as_bytes());
    Ok(result)
}

fn python_token_bytes(token: &[u8]) -> Result<Vec<u8>, VaultError> {
    // Python's non-validating URL-safe decoder ignores nonalphabet bytes but
    // still requires enough padding. Authentication remains Fernet's job.
    let mut filtered = Vec::with_capacity(token.len());
    let mut pads = 0;
    let mut complete = false;
    for &byte in token {
        let position = filtered.len() % 4;
        if byte == b'=' {
            if position >= 2 {
                pads += 1;
                if position + pads >= 4 {
                    complete = true;
                    break;
                }
            }
            continue;
        }
        let byte = match byte {
            b'-' => b'+',
            b'_' => b'/',
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' => byte,
            _ => continue,
        };
        pads = 0;
        filtered.push(byte);
    }
    let required = (4 - filtered.len() % 4) % 4;
    if required == 3 || (required != 0 && !complete) {
        return Err(VaultError);
    }
    filtered.extend(std::iter::repeat_n(b'=', required));
    let decoder = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    decoder.decode(&filtered).map_err(|_| VaultError)
}

pub fn open(key: Option<&str>, data: &[u8]) -> Result<Vec<u8>, VaultError> {
    let Some(token) = data.strip_prefix(MAGIC) else {
        return Ok(data.to_vec());
    };
    let decoded = python_token_bytes(token)?;
    let canonical = general_purpose::URL_SAFE.encode(decoded);
    cipher(key)?.decrypt(&canonical).map_err(|_| VaultError)
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    #[test]
    fn plaintext_needs_no_key() {
        assert!(matches!(open(None, b"CBV1"), Ok(bytes) if bytes == b"CBV1"));
    }

    #[test]
    fn seal_and_open_use_the_expected_envelope() {
        let sealed = seal(KEY, b"synthetic").unwrap();
        assert!(sealed.starts_with(MAGIC));
        assert!(matches!(open(Some(KEY), &sealed), Ok(bytes) if bytes == b"synthetic"));
    }

    #[test]
    fn malformed_envelope_refuses_without_a_value() {
        assert!(open(Some(KEY), b"CBV1\n!").is_err());
    }
}
