use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::io::{self, Read};

#[derive(Debug)]
enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueVisitor;
        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = Value;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a supported synthetic JSON value")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
                Ok(Value::Bool(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
                if !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&v) {
                    return Err(E::custom("integer outside synthetic domain"));
                }
                Ok(Value::Integer(v))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
                if v > 9_007_199_254_740_991 {
                    return Err(E::custom("integer outside synthetic domain"));
                }
                Ok(Value::Integer(v as i64))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
                Ok(Value::String(v.to_owned()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
                Ok(Value::String(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }
                Ok(Value::Array(values))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    if values.insert(key, value).is_some() {
                        return Err(de::Error::custom("duplicate key"));
                    }
                }
                Ok(Value::Object(values))
            }
        }
        deserializer.deserialize_any(ValueVisitor)
    }
}

fn string(value: &str, output: &mut String) {
    output.push('"');
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            c if (' '..='~').contains(&c) => output.push(c),
            c => {
                let mut units = [0; 2];
                for unit in c.encode_utf16(&mut units).iter() {
                    write!(output, "\\u{unit:04x}").unwrap();
                }
            }
        }
    }
    output.push('"');
}

fn serialize(value: &Value, depth: usize, output: &mut String) -> Result<(), &'static str> {
    if depth > 4 {
        return Err("value too deep");
    }
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(v) => output.push_str(if *v { "true" } else { "false" }),
        Value::Integer(v) => write!(output, "{v}").unwrap(),
        Value::String(v) => string(v, output),
        Value::Array(values) => {
            output.push('[');
            for (i, item) in values.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                serialize(item, depth + 1, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            for (i, (key, item)) in values.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                string(key, output);
                output.push_str(": ");
                serialize(item, depth + 1, output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

fn run() -> Result<String, Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    io::stdin().take(65_537).read_to_end(&mut input)?;
    if input.len() > 65_536 {
        return Err("input too large".into());
    }
    let value: Value = serde_json::from_slice(&input)?;
    let mut serialized = String::new();
    serialize(&value, 0, &mut serialized)?;
    let digest = format!("{:x}", Sha256::digest(serialized.as_bytes()));
    let hex: String = serialized
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let result = serde_json::json!({"serialized_hex": hex, "digest": &digest[..16]}).to_string();
    if result.len() + 1 > 131_072 {
        return Err("output too large".into());
    }
    Ok(result)
}

fn main() {
    match run() {
        Ok(result) => println!("{result}"),
        Err(_) => std::process::exit(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_duplicate_keys() {
        assert!(serde_json::from_str::<Value>(r#"{"a":1,"a":2}"#).is_err());
    }
    #[test]
    fn matches_python_unicode_escaping() {
        let mut output = String::new();
        string("é😀", &mut output);
        assert_eq!(output, r#""\u00e9\ud83d\ude00""#);
    }
}
