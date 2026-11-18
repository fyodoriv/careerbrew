//! Vector set `fernet`: what `vault.py` and `backup.verify` do with Fernet
//! tokens, run with the `fernet` crate through the `careerbrew-vault` codec.
//!
//! Each input carries `cipher_file`, the key file's text: like `backup.load_key`,
//! surrounding whitespace is dropped. Ops:
//! - `open`: an envelope (`CBV1\n` + token) or old plaintext to the
//!   plaintext bytes, as `vault.open_bytes`. Like Python's decoder, bytes
//!   after a token's completed padding are ignored (the HMAC still covers the
//!   whole token);
//! - `open-strict`: a bare token, as `backup.verify` reads it: a strict
//!   base64 check first, so appended bytes fail;
//! - `seal`: each side seals `plaintext_hex` with its own code
//!   (`vault.seal_bytes`), then reports the envelope's shape and what it
//!   opens to. Tokens hold a random IV and the time, so only the shape and
//!   the plaintext are compared.
//!
//! Plaintext travels as lowercase hex.
use base64::engine::general_purpose::URL_SAFE;
use base64::Engine;
use careerbrew_vault::{open, seal, MAGIC};
use serde_json::{json, Value};

pub fn run(op: &str, input: &Value) -> Result<Value, String> {
    let key = field(input, "cipher_file")?.trim_ascii();
    match op {
        "open" => {
            let data = field(input, "data")?;
            let plain = open(Some(key), data.as_bytes()).map_err(|_| "cannot open")?;
            Ok(Value::String(hex(&plain)))
        }
        "open-strict" => {
            let token = field(input, "data")?;
            if token.is_empty() || !token.len().is_multiple_of(4) || URL_SAFE.decode(token).is_err()
            {
                return Err("not strict base64".into());
            }
            let cipher = fernet::Fernet::new(key).ok_or("bad key")?;
            let plain =
                careerbrew_vault::decrypt_like_python(&cipher, token).map_err(|_| "cannot open")?;
            Ok(Value::String(hex(&plain)))
        }
        "seal" => {
            let plain = unhex(field(input, "plaintext_hex")?)?;
            let sealed = seal(key, &plain).map_err(|_| "cannot seal")?;
            let token = sealed.strip_prefix(MAGIC).ok_or("no envelope")?;
            let raw = URL_SAFE.decode(token).map_err(|_| "token is not base64")?;
            let opened = open(Some(key), &sealed).map_err(|_| "cannot open")?;
            Ok(json!({
                "magic": true,
                "version": raw.first().copied().ok_or("empty token")?,
                "length": raw.len(),
                "opens": hex(&opened),
            }))
        }
        _ => Err("unknown op".into()),
    }
}

fn field<'a>(input: &'a Value, name: &str) -> Result<&'a str, String> {
    input[name].as_str().ok_or_else(|| format!("no {name}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err("odd hex".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| "bad hex".to_string()))
        .collect()
}
