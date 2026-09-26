// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A message's content as two columns: a code and, for a text or an unknown
//! name, the detail (specs/007-mail-storage/data-model.md, "Content codes").
//! The schema's `CHECK` lists the same codes.

use mailbag_domain::{ContentExplanation, ReceivedContent};

/// The code and the detail stored for `content`.
pub(crate) fn content_columns(content: &ReceivedContent) -> (&'static str, Option<&str>) {
    match content {
        ReceivedContent::Text(text) => ("text", Some(text)),
        ReceivedContent::Explained(explanation) => match explanation {
            ContentExplanation::NoPlainText { has_html: false } => ("plain_text_missing", None),
            ContentExplanation::NoPlainText { has_html: true } => ("html_only", None),
            ContentExplanation::Encrypted => ("encrypted", None),
            ContentExplanation::SecuredWithSMime => ("smime", None),
            ContentExplanation::UnknownCharset(name) => ("unknown_charset", Some(name)),
            ContentExplanation::UnknownEncoding(name) => ("unknown_encoding", Some(name)),
            ContentExplanation::Undecodable => ("undecodable", None),
        },
        ReceivedContent::StructureUnreadable => ("structure_unreadable", None),
        ReceivedContent::TextNotReturned => ("text_not_returned", None),
    }
}

/// The content the stored columns describe; `None` for a code this build does
/// not know, or a code without the detail it needs.
pub(crate) fn content_from_columns(code: &str, detail: Option<String>) -> Option<ReceivedContent> {
    let explained = ReceivedContent::Explained;
    Some(match (code, detail) {
        ("text", Some(text)) => ReceivedContent::Text(text),
        ("plain_text_missing", _) => explained(ContentExplanation::NoPlainText { has_html: false }),
        ("html_only", _) => explained(ContentExplanation::NoPlainText { has_html: true }),
        ("encrypted", _) => explained(ContentExplanation::Encrypted),
        ("smime", _) => explained(ContentExplanation::SecuredWithSMime),
        ("unknown_charset", Some(name)) => explained(ContentExplanation::UnknownCharset(name)),
        ("unknown_encoding", Some(name)) => explained(ContentExplanation::UnknownEncoding(name)),
        ("undecodable", _) => explained(ContentExplanation::Undecodable),
        ("structure_unreadable", _) => ReceivedContent::StructureUnreadable,
        ("text_not_returned", _) => ReceivedContent::TextNotReturned,
        _ => return None,
    })
}
