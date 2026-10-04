// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The service's JSON answers: the folder listing, the pages of a delta
//! reading, texts, one message and the error body of a refusal. Only the list's shape and each message's identifier and read state
//! are required; any other field the service leaves out, or sends in another
//! form, is left out of the message, so one odd message never fails the list
//! (specs/005-microsoft-graph-integration/research.md §4). An empty subject,
//! name or address is left out too: the window then shows its fallback, and
//! the name rule falls back to the address. A folder needs its id and name.

use crate::{
    CHANGE_FIELDS, ChangePage, FLAG_FIELDS, GraphFailure, GraphFolder, GraphMessage, Mailbox,
    MessageChange, MessageTexts, NextPage,
};
use serde_json::Value;

/// One page of the change-tracking folder listing.
pub(crate) struct FolderPage {
    /// Each entry's folder id with the folder, or `None` when the entry marks
    /// the folder removed or the folder is hidden.
    pub(crate) entries: Vec<(String, Option<GraphFolder>)>,
    /// The next page's address; `None` on the last page.
    pub(crate) next_link: Option<String>,
}

pub(crate) fn read_folder_page(answer: &[u8]) -> Result<FolderPage, GraphFailure> {
    let answer: Value = serde_json::from_slice(answer).map_err(|_| GraphFailure::InvalidReply)?;
    let entries = answer["value"]
        .as_array()
        .ok_or(GraphFailure::InvalidReply)?
        .iter()
        .map(read_folder_entry)
        .collect::<Option<Vec<_>>>()
        .ok_or(GraphFailure::InvalidReply)?;
    let next_link = owned_text(&answer["@odata.nextLink"]);
    // The last page carries the link for later changes instead; a page with
    // neither link may not be the last.
    if next_link.is_none() && !answer["@odata.deltaLink"].is_string() {
        return Err(GraphFailure::InvalidReply);
    }
    Ok(FolderPage { entries, next_link })
}

/// `None` when the entry has no id, or names a listed folder without a name.
fn read_folder_entry(entry: &Value) -> Option<(String, Option<GraphFolder>)> {
    let folder_id = entry["id"].as_str()?.to_owned();
    if entry.get("@removed").is_some() || entry["isHidden"].as_bool() == Some(true) {
        return Some((folder_id, None));
    }
    let folder = GraphFolder {
        id: folder_id.clone(),
        name: entry["displayName"].as_str()?.to_owned(),
        parent_id: owned_text(&entry["parentFolderId"]),
        well_known: None,
    };
    Some((folder_id, Some(folder)))
}

/// The id of the one folder a request by well-known name answers with.
pub(crate) fn read_folder_id(answer: &[u8]) -> Result<String, GraphFailure> {
    let answer: Value = serde_json::from_slice(answer).map_err(|_| GraphFailure::InvalidReply)?;
    owned_text(&answer["id"]).ok_or(GraphFailure::InvalidReply)
}

/// One page of a delta reading. A page with neither link, or an entry without
/// an id, is not the documented answer.
pub(crate) fn read_change_page(answer: &[u8]) -> Result<ChangePage, GraphFailure> {
    let answer: Value = serde_json::from_slice(answer).map_err(|_| GraphFailure::InvalidReply)?;
    let changes = answer["value"]
        .as_array()
        .ok_or(GraphFailure::InvalidReply)?
        .iter()
        .map(read_change)
        .collect::<Option<Vec<_>>>()
        .ok_or(GraphFailure::InvalidReply)?;
    let next = match (
        owned_text(&answer["@odata.nextLink"]),
        owned_text(&answer["@odata.deltaLink"]),
    ) {
        (Some(next_link), _) => NextPage::More(next_link),
        (None, Some(delta_link)) => NextPage::Done(delta_link),
        (None, None) => return Err(GraphFailure::InvalidReply),
    };
    Ok(ChangePage { changes, next })
}

/// An entry that carries every list field is a listed message; one marked
/// `@removed` left the folder; any other carries only what changed.
fn read_change(entry: &Value) -> Option<MessageChange> {
    let id = entry["id"].as_str()?.to_owned();
    if entry.get("@removed").is_some() {
        return Some(MessageChange::Removed(id));
    }
    if CHANGE_FIELDS.iter().all(|field| entry.get(field).is_some()) {
        return read_message(entry).map(MessageChange::Listed);
    }
    Some(MessageChange::Changed {
        id,
        is_read: entry["isRead"].as_bool(),
        flagged: read_flagged(&entry["flag"]),
        other_fields: CHANGE_FIELDS
            .iter()
            .any(|field| !FLAG_FIELDS.contains(field) && entry.get(field).is_some()),
    })
}

/// A page of texts by message id, and the next page's link.
pub(crate) fn read_text_page(
    answer: &[u8],
) -> Result<(MessageTexts, Option<String>), GraphFailure> {
    let answer: Value = serde_json::from_slice(answer).map_err(|_| GraphFailure::InvalidReply)?;
    let texts = answer["value"]
        .as_array()
        .ok_or(GraphFailure::InvalidReply)?
        .iter()
        .map(|entry| {
            let id = entry["id"].as_str()?.to_owned();
            Some((id, owned_text(&entry["body"]["content"])))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(GraphFailure::InvalidReply)?;
    Ok((texts, owned_text(&answer["@odata.nextLink"])))
}

/// One message's text.
pub(crate) fn read_message_text(answer: &[u8]) -> Result<Option<String>, GraphFailure> {
    let answer: Value = serde_json::from_slice(answer).map_err(|_| GraphFailure::InvalidReply)?;
    Ok(owned_text(&answer["body"]["content"]))
}

/// One message and the id of the folder it is in.
pub(crate) fn read_one_message(answer: &[u8]) -> Result<(GraphMessage, String), GraphFailure> {
    let answer: Value = serde_json::from_slice(answer).map_err(|_| GraphFailure::InvalidReply)?;
    let message = read_message(&answer).ok_or(GraphFailure::InvalidReply)?;
    let folder_id = owned_text(&answer["parentFolderId"]).ok_or(GraphFailure::InvalidReply)?;
    Ok((message, folder_id))
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
        flagged: read_flagged(&entry["flag"]).unwrap_or(false),
        body_preview: present_text(&entry["bodyPreview"]),
    })
}

/// Whether a follow-up flag marks the message starred; `None` without a
/// flag status.
fn read_flagged(flag: &Value) -> Option<bool> {
    flag["flagStatus"]
        .as_str()
        .map(|status| status == "flagged")
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

    fn read(answer: &str) -> Result<ChangePage, GraphFailure> {
        read_change_page(answer.as_bytes())
    }

    /// One message as `GET /me/messages/{id}` answers it.
    fn read_one(entry: &str) -> GraphMessage {
        let entry = format!(r#"{{"parentFolderId":"inbox",{}"#, &entry[1..]);
        read_one_message(entry.as_bytes())
            .expect("a valid message")
            .0
    }

    #[test]
    fn a_listed_entry_gives_every_field() {
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
                    "flag": {"flagStatus": "flagged"},
                    "bodyPreview": "The figures are attached."
                }],
                "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$skiptoken=1"
            }"#,
        )
        .expect("a valid page");
        assert_eq!(
            page.next,
            NextPage::More(
                "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$skiptoken=1"
                    .to_owned()
            )
        );
        assert_eq!(
            page.changes,
            [MessageChange::Listed(GraphMessage {
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
                flagged: true,
                body_preview: Some("The figures are attached.".to_owned()),
            })]
        );
    }

    #[test]
    fn removed_and_partial_entries_say_what_changed() {
        let page = read(
            r#"{"value":[
                {"id":"gone","@removed":{"reason":"deleted"}},
                {"id":"read","isRead":true},
                {"id":"starred","flag":{"flagStatus":"flagged"}},
                {"id":"done","flag":{"flagStatus":"complete"}},
                {"id":"unread and unstarred","isRead":false,"flag":{"flagStatus":"notFlagged"}},
                {"id":"renamed","subject":"New subject"},
                {"id":"touched"}
            ],"@odata.deltaLink":"https://example.invalid/delta?$deltatoken=2"}"#,
        )
        .expect("a valid page");
        assert_eq!(
            page.next,
            NextPage::Done("https://example.invalid/delta?$deltatoken=2".to_owned())
        );
        let changed = |id: &str, is_read, flagged, other_fields| MessageChange::Changed {
            id: id.to_owned(),
            is_read,
            flagged,
            other_fields,
        };
        // A change of the read state or the star alone is not one of other
        // fields; a follow-up flag marked complete is not a star.
        assert_eq!(
            page.changes,
            [
                MessageChange::Removed("gone".to_owned()),
                changed("read", Some(true), None, false),
                changed("starred", None, Some(true), false),
                changed("done", None, Some(false), false),
                changed("unread and unstarred", Some(false), Some(false), false),
                changed("renamed", None, None, true),
                changed("touched", None, None, false),
            ]
        );
    }

    #[test]
    fn an_empty_reading_is_an_empty_page() {
        let page = read(r#"{"value":[],"@odata.deltaLink":"https://example.invalid/d"}"#)
            .expect("a valid page");
        assert!(page.changes.is_empty());
    }

    #[test]
    fn a_page_without_either_link_or_an_entry_without_an_id_is_invalid() {
        assert_eq!(read(r#"{"value":[]}"#), Err(GraphFailure::InvalidReply));
        assert_eq!(
            read(r#"{"value":[{"isRead":false}],"@odata.deltaLink":"https://example.invalid/d"}"#),
            Err(GraphFailure::InvalidReply)
        );
    }

    #[test]
    fn a_message_without_a_read_state_or_a_folder_is_invalid() {
        assert_eq!(
            read_one_message(br#"{"id":"message-1","parentFolderId":"inbox"}"#).err(),
            Some(GraphFailure::InvalidReply)
        );
        assert_eq!(
            read_one_message(br#"{"id":"message-1","isRead":true}"#).err(),
            Some(GraphFailure::InvalidReply)
        );
    }

    #[test]
    fn a_body_without_content_has_no_text_and_an_empty_one_is_a_text() {
        assert_eq!(
            read_message_text(br#"{"body":{"contentType":"text"}}"#),
            Ok(None)
        );
        // An empty rendering is the message's text (FR-005), not a missing field.
        assert_eq!(
            read_message_text(br#"{"body":{"contentType":"text","content":""}}"#),
            Ok(Some(String::new()))
        );
    }

    #[test]
    fn an_empty_subject_name_or_address_is_left_out() {
        let message = read_one(
            r#"{"id":"message-1","isRead":false,"subject":"",
                "from":{"emailAddress":{"name":"","address":"ada@example.org"}},
                "toRecipients":[{"emailAddress":{"name":"Bo Example","address":""}}]}"#,
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
