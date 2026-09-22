// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The steps every provider's load shares: opening an account on the server,
//! and turning a message list into a batch the reader can show.

use crate::batch::{ReceivedBatch, ReceivedContent, ReceivedMessage, ServerFailure};
use goa_adapter::{AccountId, ImapAccess, ImapCredential, ImapEncryption};
use mailbag_content::{
    ContentExplanation, MimePart, TextSelection, decode_display_fields, decode_text_part,
    join_message_text, select_text_parts,
};
use mailbag_imap::{
    Credential, Encryption, ImapAccount, ImapFailure, InboxReader, MessageList, MessagePart,
    MessageText, TextParts, TextRequest,
};
use std::collections::BTreeMap;

pub(crate) fn server_account(access: ImapAccess) -> ImapAccount {
    ImapAccount {
        host: access.host,
        login: access.login,
        credential: match access.credential {
            ImapCredential::Password(password) => Credential::Password(password),
            ImapCredential::AccessToken(token) => Credential::AccessToken(token),
        },
        encryption: match access.encryption {
            ImapEncryption::ImplicitTls => Encryption::ImplicitTls,
            ImapEncryption::StartTls => Encryption::StartTls,
        },
    }
}

/// Reads the part structures of the listed messages, decodes the text each one
/// needs and assembles the batch. Every provider reaches this with its own
/// message list; nothing below here is provider-specific.
pub(crate) async fn load_batch_from_rows(
    reader: &mut InboxReader,
    listed: MessageList,
    account_id: AccountId,
) -> Result<ReceivedBatch, ServerFailure> {
    let rows = listed.rows;
    let window = rows.len();
    let uids: Vec<u32> = rows.iter().map(|row| row.uid).collect();
    let structures = reader.fetch_structures(&uids).await?;

    let mut selections = BTreeMap::new();
    let mut requests = Vec::new();
    for (uid, structure) in &structures {
        let Some(part) = structure else {
            continue;
        };
        let selection = tracing::debug_span!("message", uid)
            .in_scope(|| select_text_parts(&describe_part(part)));
        if let TextSelection::Parts(sections) = &selection {
            requests.push(TextRequest {
                uid: *uid,
                parts: text_parts(part, sections),
            });
        }
        selections.insert(*uid, selection);
    }

    // Each message's text is decoded as the reader reports it, and the raw
    // MIME is released with the request group it belongs to.
    let mut texts = BTreeMap::new();
    reader
        .fetch_text(requests, |uid, text| {
            let _message = tracing::debug_span!("message", uid).entered();
            // A message absent here disappeared from the Inbox during the load.
            if let Some(content) = decode_message_text(&text) {
                texts.insert(uid, content);
            }
        })
        .await?;

    let messages: Vec<ReceivedMessage> = rows
        .into_iter()
        .filter_map(|row| {
            let content = match structures.get(&row.uid)? {
                // The server could not describe this message.
                None => ReceivedContent::Explained(ContentExplanation::UnreadableStructure),
                Some(_) => match selections.get(&row.uid)? {
                    TextSelection::Explained(explanation) => {
                        ReceivedContent::Explained(explanation.clone())
                    }
                    // A message missing here disappeared during the load.
                    TextSelection::Parts(_) => texts.remove(&row.uid)?,
                },
            };
            Some(ReceivedMessage {
                uid: row.uid,
                fields: tracing::debug_span!("message", uid = row.uid)
                    .in_scope(|| decode_display_fields(&row.list_headers)),
                internal_date: row.internal_date,
                seen: row.seen,
                content,
                gmail: None,
            })
        })
        .collect();
    // Every message of a window that was not empty disappeared, for example
    // because another client moved them. Older mail outside the window may
    // still be there, so this is not an empty Inbox.
    if messages.is_empty() && window > 0 {
        return Err(ImapFailure::InboxChanged.into());
    }
    Ok(ReceivedBatch {
        account_id,
        uid_validity: reader.uid_validity(),
        messages,
        list_refusal: listed.refusal,
    })
}

/// The part description the content rules walk. Both trees have the same
/// shape, so a selected part's path is its IMAP section number.
fn describe_part(part: &MessagePart) -> MimePart {
    MimePart {
        section: part.section.clone(),
        media_type: part.media_type.clone(),
        media_subtype: part.media_subtype.clone(),
        parameters: part.parameters.clone(),
        disposition: part.disposition.clone(),
        content_id: part.content_id.clone(),
        children: part.children.iter().map(describe_part).collect(),
    }
}

/// How the selected sections are read: a single-part message needs the
/// message header, a multipart leaf its own MIME header.
fn text_parts(root: &MessagePart, sections: &[Vec<u32>]) -> TextParts {
    match root.children.is_empty() {
        true => TextParts::SinglePartBody,
        false => TextParts::MultipartLeaves(sections.to_vec()),
    }
}

/// Decodes one message's received text, or explains why there is none.
/// `None` means the message disappeared from the Inbox.
fn decode_message_text(text: &MessageText) -> Option<ReceivedContent> {
    let parts = match text {
        MessageText::Received(parts) => parts,
        MessageText::NotReturned => {
            return Some(ReceivedContent::Explained(
                ContentExplanation::TextNotReturned,
            ));
        }
        MessageText::Disappeared => return None,
    };
    let decoded: Result<Vec<String>, ContentExplanation> = parts
        .iter()
        .map(|part| decode_text_part(&part.header, &part.body))
        .collect();
    Some(match decoded {
        Ok(texts) => ReceivedContent::Text(join_message_text(&texts)),
        // One unreadable part leaves no complete text to show.
        Err(explanation) => ReceivedContent::Explained(explanation),
    })
}
