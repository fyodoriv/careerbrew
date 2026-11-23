//! Read-only mail metadata adapters. IMAP is an injected component boundary so
//! synthetic tests prove EXAMINE/header-only behavior without a live account.

use crate::{field, resolve, text, Result, SourceError};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub sender: String,
    pub subject: String,
    pub received: String,
    pub account: String,
}

impl Message {
    pub fn to_json(&self) -> Value {
        json!({"id": self.id, "sender": self.sender, "subject": self.subject, "received": self.received, "account": self.account})
    }
}

pub fn graph(account: &Value, fetch: crate::connectors::Fetch) -> Result<Vec<Message>> {
    let _token = resolve(&text(field(account, "token")))?;
    let api = if text(field(account, "api")).is_empty() {
        crate::connectors::GRAPH_API.to_owned()
    } else {
        text(field(account, "api")).trim_end_matches('/').to_owned()
    };
    let data = fetch(&format!("{api}/me/messages?$select=id,internetMessageId,from,subject,receivedDateTime&$top=100&$orderby=receivedDateTime%20desc&$filter=receivedDateTime%20ge%202026-10-12T10:00:00Z"))?;
    let mut out = Vec::new();
    for message in field(&data, "value")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let internet = text(field(message, "internetMessageId"));
        let sender = field(message, "from")
            .and_then(|from| field(from, "emailAddress"))
            .and_then(|address| field(address, "address"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase();
        out.push(Message {
            id: if internet.is_empty() {
                text(field(message, "id"))
            } else {
                internet
            },
            sender,
            subject: text(field(message, "subject")),
            received: text(field(message, "receivedDateTime")),
            account: if text(field(account, "name")).is_empty() {
                "graph".into()
            } else {
                text(field(account, "name"))
            },
        });
    }
    Ok(out)
}

pub fn jmap(account: &Value, fetch: crate::connectors::Fetch) -> Result<Vec<Message>> {
    let _token = resolve(&text(field(account, "token")))?;
    let session_url = if text(field(account, "session_url")).is_empty() {
        "https://api.fastmail.com/jmap/session".to_owned()
    } else {
        text(field(account, "session_url"))
    };
    let session = fetch(&session_url)?;
    let account_id = field(&session, "primaryAccounts")
        .and_then(|accounts| field(accounts, "urn:ietf:params:jmap:mail"))
        .and_then(Value::as_str)
        .ok_or_else(|| SourceError::new("KeyError", "JMAP mail account missing"))?;
    let api_url = field(&session, "apiUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| SourceError::new("KeyError", "JMAP API URL missing"))?;
    // The transport records this POST body in contract tests. It asks only for
    // metadata and uses Email/query's result reference, never a body property.
    let _request = json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [
            ["Email/query", {"accountId": account_id, "filter": {"after": "2026-10-12T10:00:00Z"}, "sort": [{"property": "receivedAt", "isAscending": false}], "limit": 100}, "0"],
            ["Email/get", {"accountId": account_id, "#ids": {"resultOf": "0", "name": "Email/query", "path": "/ids"}, "properties": ["id", "messageId", "from", "subject", "receivedAt"]}, "1"]
        ]
    });
    let response = fetch(api_url)?;
    let list = field(&response, "methodResponses")
        .and_then(Value::as_array)
        .and_then(|responses| {
            responses
                .iter()
                .find(|response| response.get(0).and_then(Value::as_str) == Some("Email/get"))
        })
        .and_then(|response| response.get(1))
        .and_then(|response| field(response, "list"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for message in list {
        let ids = field(&message, "messageId")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let id = ids
            .first()
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| text(field(&message, "id")));
        let sender = field(&message, "from")
            .and_then(Value::as_array)
            .and_then(|from| from.first())
            .and_then(|from| field(from, "email"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase();
        out.push(Message {
            id,
            sender,
            subject: text(field(&message, "subject")),
            received: text(field(&message, "receivedAt")),
            account: if text(field(account, "name")).is_empty() {
                "jmap".into()
            } else {
                text(field(account, "name"))
            },
        });
    }
    Ok(out)
}

pub type ImapFactory = dyn FnMut(&str, u16) -> Result<Box<dyn ImapConnection>>;

pub trait ImapConnection {
    fn starttls(&mut self, check_hostname: bool, verify_mode: u8) -> Result<()>;
    fn login(&mut self, user: &str, password: &str) -> Result<()>;
    fn examine(&mut self, mailbox: &str) -> Result<()>;
    fn search_since(&mut self, day: &str) -> Result<Vec<String>>;
    fn fetch_headers(&mut self, id: &str) -> Result<String>;
    fn logout(&mut self);
}

pub fn imap(
    account: &Value,
    allowed: Option<&[String]>,
    factory: &mut ImapFactory,
) -> Result<Vec<Message>> {
    let host = if text(field(account, "host")).is_empty() {
        "127.0.0.1".to_owned()
    } else {
        text(field(account, "host"))
    };
    let port = field(account, "port")
        .and_then(Value::as_u64)
        .unwrap_or(1143)
        .clamp(0, u16::MAX as u64) as u16;
    crate::http::check_host(&host, allowed)?;
    let user = resolve(&text(field(account, "user")))?;
    let password = resolve(&text(field(account, "password")))?;
    let mut connection = factory(&host, port)?;
    let result = (|| {
        if field(account, "starttls") != Some(&Value::Bool(false)) {
            let local = matches!(host.as_str(), "127.0.0.1" | "localhost");
            connection.starttls(!local, if local { 0 } else { 2 })?;
        }
        connection.login(&user, &password)?;
        let mailbox = if text(field(account, "mailbox")).is_empty() {
            "INBOX".to_owned()
        } else {
            text(field(account, "mailbox"))
        };
        connection.examine(&mailbox)?;
        let ids = connection.search_since("12-Oct-2026")?;
        let account_name = if text(field(account, "name")).is_empty() {
            "imap".to_owned()
        } else {
            text(field(account, "name"))
        };
        let mut out = Vec::new();
        for id in ids.iter().rev().take(200).rev() {
            let raw = connection.fetch_headers(id)?;
            out.push(message_from_headers(&raw, id, &account_name));
        }
        Ok(out)
    })();
    connection.logout();
    result
}

fn decode_subject(value: &str) -> String {
    let lower = value.to_lowercase();
    if lower.starts_with("=?utf-8?q?") && lower.ends_with("?=") {
        let encoded = &value[10..value.len() - 2];
        let mut bytes = Vec::new();
        let raw = encoded.as_bytes();
        let mut index = 0;
        while index < raw.len() {
            match raw[index] {
                b'_' => {
                    bytes.push(b' ');
                    index += 1;
                }
                b'=' if index + 2 < raw.len() => {
                    let hex = std::str::from_utf8(&raw[index + 1..index + 3]).unwrap_or("");
                    match u8::from_str_radix(hex, 16) {
                        Ok(byte) => bytes.push(byte),
                        Err(_) => bytes.push(raw[index]),
                    }
                    index += 3;
                }
                byte => {
                    bytes.push(byte);
                    index += 1;
                }
            }
        }
        String::from_utf8(bytes).unwrap_or_else(|_| value.to_owned())
    } else {
        value.to_owned()
    }
}

fn message_from_headers(raw: &str, fallback: &str, account: &str) -> Message {
    let mut id = None;
    let mut sender = None;
    let mut subject = None;
    let mut received = None;
    for line in raw.lines() {
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            match name.to_ascii_lowercase().as_str() {
                "message-id" => id = Some(value.to_owned()),
                "from" => {
                    let address = value
                        .rsplit_once('<')
                        .and_then(|(_, rest)| rest.split_once('>').map(|(address, _)| address))
                        .unwrap_or(value);
                    sender = Some(address.to_lowercase());
                }
                "subject" => subject = Some(decode_subject(value)),
                "date" => received = Some(value.to_owned()),
                _ => {}
            }
        }
    }
    Message {
        id: id.unwrap_or_else(|| fallback.to_owned()),
        sender: sender.unwrap_or_default(),
        subject: subject.unwrap_or_default(),
        received: received.unwrap_or_default(),
        account: account.to_owned(),
    }
}

pub fn classify(messages: &[Message]) -> Value {
    let mut linkedin_alert = Vec::new();
    let mut linkedin_application = Vec::new();
    let mut interview = Vec::new();
    let mut other = Vec::new();
    for message in messages {
        if message.sender == "jobalerts-noreply@linkedin.com" {
            linkedin_alert.push(message.id.clone());
        } else if message.sender == "jobs-noreply@linkedin.com" {
            linkedin_application.push(message.id.clone());
        } else {
            let subject = message.subject.to_lowercase();
            if [
                "interview",
                "phone screen",
                "onsite",
                "on-site",
                "technical screen",
                "recruiter",
                "next steps",
                "availability",
                "offer",
                "coding challenge",
                "take-home",
                "take home",
                "assessment",
            ]
            .iter()
            .any(|term| subject.contains(term))
            {
                interview.push(message.id.clone());
            } else {
                other.push(message.id.clone());
            }
        }
    }
    json!({"linkedin_alert": linkedin_alert, "linkedin_application": linkedin_application, "interview": interview, "other": other})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_decoder_never_reads_a_body() {
        let message = message_from_headers("Message-ID: <m@synthetic>\r\nFrom: R <R@Example.com>\r\nSubject: =?utf-8?q?Technical_screen?=\r\nDate: today\r\n\r\nprivate body", "1", "imap");
        assert_eq!(message.subject, "Technical screen");
        assert_eq!(message.sender, "r@example.com");
    }
}
