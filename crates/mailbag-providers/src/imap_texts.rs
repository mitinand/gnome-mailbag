// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What both IMAP providers share, Generic IMAP and Gmail: the account as the
//! protocol crate takes it, and reading the text of the messages a cycle
//! downloads, which the content rules choose from each message's parts.

use goa_adapter::{ImapAccess, ImapCredential, ImapEncryption};
use mailbag_content::{MimePart, TextSelection, decode_message_text, select_text_parts};
use mailbag_domain::ReceivedContent;
use mailbag_imap::{
    Credential, Encryption, ImapAccount, ImapError, MailboxReader, MessagePart, MessageText,
    TextParts, TextRequest,
};
use std::collections::BTreeMap;

pub(crate) fn imap_account(access: ImapAccess) -> ImapAccount {
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

/// Reads what the reader shows of each message: its text, or why it has
/// none. A message missing from the result disappeared from the folder
/// meanwhile. Structures and texts are asked for these messages only, never
/// for a whole folder (specs/009-synchronization/research.md §3).
pub(crate) async fn read_contents(
    reader: &mut MailboxReader,
    uids: &[u32],
) -> Result<BTreeMap<u32, ReceivedContent>, ImapError> {
    let structures = reader.fetch_structures(uids).await?;
    let mut contents = BTreeMap::new();
    let mut requests = Vec::new();
    for (uid, structure) in &structures {
        let Some(part) = structure else {
            // The server could not describe this message.
            contents.insert(*uid, ReceivedContent::StructureUnreadable);
            continue;
        };
        let selection = tracing::debug_span!("message", uid)
            .in_scope(|| select_text_parts(&describe_part(part)));
        match selection {
            TextSelection::Explained(explanation) => {
                contents.insert(*uid, ReceivedContent::Explained(explanation));
            }
            TextSelection::Parts(sections) => requests.push(TextRequest {
                uid: *uid,
                parts: text_parts(part, &sections),
            }),
        }
    }
    // Each message's text is decoded as the reader reports it, and the raw
    // MIME is released with the request group it belongs to.
    reader
        .fetch_text(requests, |uid, text| {
            let _message = tracing::debug_span!("message", uid).entered();
            if let Some(content) = received_text(&text) {
                contents.insert(uid, content);
            }
        })
        .await?;
    Ok(contents)
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

/// One message's received text, or why there is none. `None` means the
/// message disappeared from the folder.
fn received_text(text: &MessageText) -> Option<ReceivedContent> {
    Some(match text {
        MessageText::Received(parts) => {
            let parts = parts
                .iter()
                .map(|part| (part.header.as_slice(), part.body.as_slice()));
            match decode_message_text(parts) {
                Ok(text) => ReceivedContent::Text(text),
                Err(explanation) => ReceivedContent::Explained(explanation),
            }
        }
        MessageText::NotReturned => ReceivedContent::TextNotReturned,
        MessageText::Disappeared => return None,
    })
}
