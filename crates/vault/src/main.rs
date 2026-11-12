use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Deserializer};
use std::io::{self, Read};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Single {
    op: String,
    data: String,
    key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchItem {
    id: i64,
    op: String,
    data: String,
    #[serde(deserialize_with = "required_key")]
    key: Option<String>,
}

fn required_key<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    requests: Vec<BatchItem>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Request {
    Batch(Batch),
    Single(Single),
}

fn run() -> Result<String, ()> {
    let mut input = Vec::new();
    io::stdin()
        .take(32769)
        .read_to_end(&mut input)
        .map_err(|_| ())?;
    if input.len() > 32768 {
        return Err(());
    }
    let request: Request = serde_json::from_slice(&input).map_err(|_| ())?;
    let output = match request {
        Request::Batch(batch) => {
            if batch.requests.is_empty() || batch.requests.len() > 32 {
                return Err(());
            }
            let mut ids = std::collections::HashSet::new();
            let mut responses = Vec::with_capacity(batch.requests.len());
            for item in batch.requests {
                if !ids.insert(item.id) {
                    return Err(());
                }
                let data = parse_data(&item.op, &item.data)?;
                let response = match operate(&item.op, &data, item.key.as_deref()) {
                    Ok(result) => {
                        serde_json::json!({"id": item.id, "status": "ok", "data": STANDARD.encode(result)})
                    }
                    Err(()) => serde_json::json!({"id": item.id, "status": "refused"}),
                };
                responses.push(response);
            }
            serde_json::json!({"responses": responses}).to_string()
        }
        Request::Single(item) => {
            let data = parse_data(&item.op, &item.data)?;
            let result = operate(&item.op, &data, item.key.as_deref())?;
            serde_json::json!({"data": STANDARD.encode(result)}).to_string()
        }
    };
    if output.len() + 1 > 32768 {
        return Err(());
    }
    Ok(output)
}

fn parse_data(op: &str, data: &str) -> Result<Vec<u8>, ()> {
    if !matches!(op, "seal" | "open") {
        return Err(());
    }
    STANDARD.decode(data).map_err(|_| ())
}

fn operate(op: &str, data: &[u8], key: Option<&str>) -> Result<Vec<u8>, ()> {
    match op {
        "seal" => careerbrew_vault::seal(key.ok_or(())?, data),
        "open" => careerbrew_vault::open(key, data),
        _ => return Err(()),
    }
    .map_err(|_| ())
}

fn main() {
    match run() {
        Ok(output) => println!("{output}"),
        Err(()) => std::process::exit(1),
    }
}
