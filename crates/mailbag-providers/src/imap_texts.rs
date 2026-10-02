// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What both IMAP providers share, Generic IMAP and Gmail: the account as the
//! protocol crate takes it, and reading the text of the messages a cycle
//! downloads, which the content rules choose from each message's parts.

use goa_adapter::{ImapAccess, ImapCredential, ImapEncryption};
use mailbag_content::{
    MimePart, PREVIEW_PIECE_BYTES, TextSelection, decode_message_text, preview_of_piece,
    preview_of_text, select_preview_part, select_text_parts,
};
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

/// Reads what the reader shows of each message and its preview. A recent
/// message gets its text, or why it has none; every message gets the
/// beginning of its page or plain part for the preview
/// (specs/010-message-list/research.md §3). A message missing from the
/// result disappeared from the folder meanwhile. Structures and texts are
/// asked for these messages only, never for a whole folder
/// (specs/009-synchronization/research.md §3).
pub(crate) async fn read_contents(
    reader: &mut MailboxReader,
    uids: &[u32],
    recent: &[u32],
) -> Result<BTreeMap<u32, (ReceivedContent, String)>, ImapError> {
    let structures = reader.fetch_structures(uids).await?;
    let mut readings: BTreeMap<u32, MessageReading> = structures
        .iter()
        .map(|(uid, structure)| {
            let _message = tracing::debug_span!("message", uid).entered();
            let reading = plan_reading(structure.as_ref(), recent.contains(uid));
            (*uid, reading)
        })
        .collect();
    let requests = readings
        .iter()
        .flat_map(|(uid, reading)| reading.requests(*uid))
        .collect();
    // Each message's text is decoded as the reader reports it, and the raw
    // MIME is released with the request group it belongs to.
    reader
        .fetch_text(requests, |uid, limit, text| {
            let _message = tracing::debug_span!("message", uid).entered();
            if let Some(reading) = readings.get_mut(&uid) {
                reading.receive(limit, text);
            }
        })
        .await?;
    let contents: BTreeMap<u32, (ReceivedContent, String)> = readings
        .into_iter()
        .filter_map(|(uid, reading)| {
            let _message = tracing::debug_span!("message", uid).entered();
            Some((uid, reading.finish()?))
        })
        .collect();
    tracing::info!(
        messages = contents.len(),
        empty_previews = contents
            .values()
            .filter(|(_, preview)| preview.is_empty())
            .count(),
        "previews made"
    );
    Ok(contents)
}

/// What a cycle reads of one message, and what it received of it.
struct MessageReading {
    /// What the reader shows; replaced when the text is received.
    content: ReceivedContent,
    /// The parts of a recent message's text, read whole.
    text_parts: Option<TextParts>,
    /// The parts whose beginnings the preview is made from, in the order
    /// they are tried.
    piece_parts: Option<TextParts>,
    /// Whether the first piece is a web page; any other piece is plain text.
    page_first: bool,
    /// Whether the page is read whole as the last of the text parts: one
    /// read of a message costs a server about as much as a piece of it.
    page_with_text: bool,
    pieces: Option<MessageText>,
    disappeared: bool,
}

/// Chooses what to read of a message: a recent one's text, and the part
/// its preview is made from, with the plain part as well for an old
/// message whose page may yield no words. A recent message's plain text is
/// read whole anyway, so its preview needs no piece of it, and its page is
/// read whole with it in the same request: a server such as iCloud spends
/// most of a read on opening the message, not on its size
/// (specs/010-message-list/research.md §3).
fn plan_reading(structure: Option<&MessagePart>, is_recent: bool) -> MessageReading {
    let mut reading = MessageReading {
        content: ReceivedContent::NotDownloaded,
        text_parts: None,
        piece_parts: None,
        page_first: false,
        page_with_text: false,
        pieces: None,
        disappeared: false,
    };
    let Some(root) = structure else {
        // The server could not describe this message.
        if is_recent {
            reading.content = ReceivedContent::StructureUnreadable;
        }
        return reading;
    };
    let described = describe_part(root);
    let plain_sections = match select_text_parts(&described) {
        TextSelection::Parts(sections) => sections,
        TextSelection::Explained(explanation) => {
            if is_recent {
                reading.content = ReceivedContent::Explained(explanation);
            }
            Vec::new()
        }
    };
    if is_recent && !plain_sections.is_empty() {
        reading.content = ReceivedContent::TextNotReturned;
        reading.text_parts = Some(text_parts(root, &plain_sections));
    }
    let preview_part = select_preview_part(&described);
    reading.page_first = preview_part.as_ref().is_some_and(|part| part.is_html);
    let piece_sections = match preview_part {
        Some(page) if page.is_html => {
            let plain_fallback = plain_sections.first().filter(|_| !is_recent);
            [page.section]
                .into_iter()
                .chain(plain_fallback.cloned())
                .collect()
        }
        Some(_) if is_recent => Vec::new(),
        Some(plain) => vec![plain.section],
        None => Vec::new(),
    };
    match (&reading.text_parts, piece_sections.as_slice()) {
        (Some(_), [page]) if reading.page_first => {
            let sections = [plain_sections.as_slice(), std::slice::from_ref(page)].concat();
            reading.text_parts = Some(text_parts(root, &sections));
            reading.page_with_text = true;
        }
        (_, []) => {}
        _ => reading.piece_parts = Some(text_parts(root, &piece_sections)),
    }
    reading
}

impl MessageReading {
    fn requests(&self, uid: u32) -> Vec<TextRequest> {
        let text = self.text_parts.clone().map(|parts| TextRequest {
            uid,
            parts,
            limit: None,
        });
        let pieces = self.piece_parts.clone().map(|parts| TextRequest {
            uid,
            parts,
            limit: Some(PREVIEW_PIECE_BYTES),
        });
        text.into_iter().chain(pieces).collect()
    }

    fn receive(&mut self, limit: Option<u32>, text: MessageText) {
        match (limit, &text) {
            (_, MessageText::Disappeared) => self.disappeared = true,
            (None, MessageText::Received(parts)) if self.page_with_text => {
                let mut parts = parts.clone();
                let page = parts.pop().expect("the page is the last part read");
                self.pieces = Some(MessageText::Received(vec![page]));
                if let Some(content) = received_text(&MessageText::Received(parts)) {
                    self.content = content;
                }
            }
            (None, _) => {
                if let Some(content) = received_text(&text) {
                    self.content = content;
                }
            }
            (Some(_), _) => self.pieces = Some(text),
        }
    }

    /// The content and the preview, or `None` when the message disappeared.
    /// The preview is the first source that yields words: the page's piece,
    /// the plain part's piece, then a recent message's whole text.
    fn finish(self) -> Option<(ReceivedContent, String)> {
        if self.disappeared {
            return None;
        }
        let pieces = match &self.pieces {
            Some(MessageText::Received(parts)) => parts.as_slice(),
            _ => &[],
        };
        let from_pieces = pieces.iter().enumerate().map(|(index, piece)| {
            preview_of_piece(&piece.header, &piece.body, index == 0 && self.page_first)
        });
        let from_text = match &self.content {
            ReceivedContent::Text(text) => Some(preview_of_text(text)),
            _ => None,
        };
        let preview = from_pieces
            .chain(from_text)
            .find(|preview| !preview.is_empty())
            .unwrap_or_default();
        Some((self.content, preview))
    }
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
