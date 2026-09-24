// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The service's JSON answers: the message list and the error body of a
//! refusal. Only the list's shape and each message's identifier and read state
//! are required; any other field the service leaves out, or sends in another
//! form, is left out of the message, so one odd message never fails the list
//! (specs/005-microsoft-graph-integration/research.md §4). An empty subject,
//! name or address is left out too: the window then shows its fallback, and
//! the name rule falls back to the address.

use crate::{GraphFailure, GraphMessage, InboxPage, Mailbox};
use serde_json::Value;

pub(crate) fn read_inbox_page(answer: &[u8]) -> Result<InboxPage, GraphFailure> {
    let answer: Value = serde_json::from_slice(answer).map_err(|_| GraphFailure::InvalidReply)?;
    let entries = answer["value"]
        .as_array()
        .ok_or(GraphFailure::InvalidReply)?;
    let messages = entries
        .iter()
        .map(read_message)
        .collect::<Option<Vec<_>>>()
        .ok_or(GraphFailure::InvalidReply)?;
    Ok(InboxPage {
        messages,
        more_available: answer["@odata.nextLink"].is_string(),
    })
}

/// The error code and the developer message of a refusal, when the body is
/// the service's documented error object.
pub(crate) fn read_error(answer: &[u8]) -> Option<(Option<String>, Option<String>)> {
    let answer: Value = serde_json::from_slice(answer).ok()?;
    let error = answer.get("error").filter(|error| error.is_object())?;
    Some((owned_text(&error["code"]), owned_text(&error["message"])))
}

/// `None` when the entry has no identifier or read state.
fn read_message(entry: &Value) -> Option<GraphMessage> {
    Some(GraphMessage {
        immutable_id: entry["id"].as_str()?.to_owned(),
        subject: present_text(&entry["subject"]),
        from: read_mailbox(&entry["from"]),
        to: entry["toRecipients"]
            .as_array()
            .map(|recipients| recipients.iter().filter_map(read_mailbox).collect())
            .unwrap_or_default(),
        received_unix: entry["receivedDateTime"].as_str().and_then(unix_seconds),
        is_read: entry["isRead"].as_bool()?,
        body_text: owned_text(&entry["body"]["content"]),
    })
}

fn read_mailbox(recipient: &Value) -> Option<Mailbox> {
    let email_address = recipient.get("emailAddress")?;
    Some(Mailbox {
        name: present_text(&email_address["name"]),
        address: present_text(&email_address["address"]),
    })
}

/// A string field with text in it; an empty string carries nothing to show.
fn present_text(field: &Value) -> Option<String> {
    owned_text(field).filter(|text| !text.is_empty())
}

fn owned_text(field: &Value) -> Option<String> {
    field.as_str().map(str::to_owned)
}

/// The service writes times in ISO 8601, in UTC.
fn unix_seconds(iso_8601: &str) -> Option<i64> {
    glib::DateTime::from_iso8601(iso_8601, None)
        .ok()
        .map(|time| time.to_unix())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(answer: &str) -> Result<InboxPage, GraphFailure> {
        read_inbox_page(answer.as_bytes())
    }

    fn read_one(entry: &str) -> GraphMessage {
        let page = read(&format!(r#"{{"value":[{entry}]}}"#)).expect("a valid page");
        page.messages.into_iter().next().expect("one message")
    }

    #[test]
    fn the_documented_answer_gives_every_field() {
        let page = read(
            r#"{
                "@odata.context": "https://graph.microsoft.com/v1.0/$metadata#Collection(message)",
                "value": [{
                    "@odata.etag": "W/\"CQAAABYAAAA\"",
                    "id": "AAkALgAAAAAAHYQDEapmEc2byACqAC-EWg0A",
                    "subject": "Quarterly figures",
                    "from": {"emailAddress": {"name": "Ada Example", "address": "ada@example.org"}},
                    "toRecipients": [
                        {"emailAddress": {"name": "Bo Example", "address": "bo@example.org"}},
                        {"emailAddress": {"address": "cy@example.org"}}
                    ],
                    "receivedDateTime": "2018-09-09T03:15:08Z",
                    "isRead": true,
                    "body": {"contentType": "text", "content": "Figures attached."}
                }],
                "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages?$skip=1"
            }"#,
        )
        .expect("a valid page");
        assert!(page.more_available);
        assert_eq!(
            page.messages,
            [GraphMessage {
                immutable_id: "AAkALgAAAAAAHYQDEapmEc2byACqAC-EWg0A".to_owned(),
                subject: Some("Quarterly figures".to_owned()),
                from: Some(Mailbox {
                    name: Some("Ada Example".to_owned()),
                    address: Some("ada@example.org".to_owned()),
                }),
                to: vec![
                    Mailbox {
                        name: Some("Bo Example".to_owned()),
                        address: Some("bo@example.org".to_owned()),
                    },
                    Mailbox {
                        name: None,
                        address: Some("cy@example.org".to_owned()),
                    },
                ],
                received_unix: Some(1_536_462_908),
                is_read: true,
                body_text: Some("Figures attached.".to_owned()),
            }]
        );
    }

    #[test]
    fn an_empty_inbox_is_an_empty_page() {
        assert_eq!(
            read(r#"{"value":[]}"#),
            Ok(InboxPage {
                messages: Vec::new(),
                more_available: false,
            })
        );
    }

    #[test]
    fn a_message_without_an_identifier_fails_the_page() {
        assert_eq!(
            read(r#"{"value":[{"isRead":false}]}"#),
            Err(GraphFailure::InvalidReply)
        );
    }

    #[test]
    fn a_message_without_a_read_state_fails_the_page() {
        assert_eq!(
            read(r#"{"value":[{"id":"message-1"}]}"#),
            Err(GraphFailure::InvalidReply)
        );
    }

    #[test]
    fn a_body_without_content_leaves_the_text_out() {
        let message =
            read_one(r#"{"id":"message-1","isRead":false,"body":{"contentType":"text"}}"#);
        assert_eq!(message.body_text, None);
    }

    #[test]
    fn an_empty_subject_name_or_address_is_left_out() {
        let message = read_one(
            r#"{"id":"message-1","isRead":false,"subject":"",
                "from":{"emailAddress":{"name":"","address":"ada@example.org"}},
                "toRecipients":[{"emailAddress":{"name":"Bo Example","address":""}}],
                "body":{"contentType":"text","content":""}}"#,
        );
        assert_eq!(message.subject, None);
        assert_eq!(
            message.from,
            Some(Mailbox {
                name: None,
                address: Some("ada@example.org".to_owned()),
            })
        );
        assert_eq!(
            message.to,
            [Mailbox {
                name: Some("Bo Example".to_owned()),
                address: None,
            }]
        );
        // An empty rendering is the message's text (FR-005), not a missing field.
        assert_eq!(message.body_text, Some(String::new()));
    }

    #[test]
    fn an_empty_recipient_list_gives_no_recipients() {
        let message = read_one(r#"{"id":"message-1","isRead":false,"toRecipients":[]}"#);
        assert_eq!(message.to, []);
    }

    #[test]
    fn a_date_the_parser_rejects_leaves_the_time_out() {
        let message =
            read_one(r#"{"id":"message-1","isRead":false,"receivedDateTime":"yesterday"}"#);
        assert_eq!(message.received_unix, None);
    }

    #[test]
    fn an_answer_that_is_not_an_object_fails_the_page() {
        assert_eq!(
            read(r#"[{"id":"message-1"}]"#),
            Err(GraphFailure::InvalidReply)
        );
        assert_eq!(read("not JSON"), Err(GraphFailure::InvalidReply));
    }

    #[test]
    fn an_error_body_gives_its_code_and_message() {
        let body = br#"{"error":{"code":"InvalidAuthenticationToken","message":"Access token has expired.","innerError":{"date":"2026-09-23T08:00:00"}}}"#;
        assert_eq!(
            read_error(body),
            Some((
                Some("InvalidAuthenticationToken".to_owned()),
                Some("Access token has expired.".to_owned())
            ))
        );
        assert_eq!(read_error(b"Service Unavailable"), None);
    }
}
