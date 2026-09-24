// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Chooses the readable text parts of a received message and decodes them.
//!
//! This crate works on MIME part descriptions and raw MIME entities through
//! mail-parser. It knows nothing about IMAP, GIO or the user interface.

#[cfg(test)]
mod tests;

use mail_parser::{MessageParser, MimeHeaders, PartType, decoders::charsets::map::charset_decoder};

/// One part of a message's MIME structure, as the server described it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MimePart {
    /// IMAP section numbers: `[]` for a multipart message, `[1]` for the body
    /// of a single-part message and, for example, `[2, 1]` inside a multipart.
    pub section: Vec<u32>,
    /// Lowercase media type and subtype, such as `text` and `plain`.
    pub media_type: String,
    pub media_subtype: String,
    /// Content-Type parameters with lowercase names, such as `charset`.
    pub parameters: Vec<(String, String)>,
    /// Lowercase Content-Disposition type, such as `attachment`, if any.
    pub disposition: Option<String>,
    /// Content-ID without angle brackets, by which a related set names its
    /// root. A server reports it for single parts only.
    pub content_id: Option<String>,
    /// Parts of a multipart. Nested messages are not expanded.
    pub children: Vec<MimePart>,
}

impl MimePart {
    fn parameter(&self, name: &str) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(parameter, _)| parameter == name)
            .map(|(_, value)| value.as_str())
    }

    /// Whether the part carries a file name. A server may send it in any form
    /// RFC 2231 allows and need not fold the pieces back together, so the
    /// extended `name*` and continuations such as `name*0*` count as well.
    fn has_file_name(&self) -> bool {
        self.parameters
            .iter()
            .any(|(parameter, _)| is_file_name_parameter(parameter))
    }

    fn disposition_is(&self, disposition: &str) -> bool {
        self.disposition.as_deref() == Some(disposition)
    }
}

/// The text parts to read from a message, or why there are none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextSelection {
    /// Section numbers of the plain-text parts, in reading order.
    Parts(Vec<Vec<u32>>),
    Explained(ContentExplanation),
}

/// Why a message shows no text, in terms the reader explains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentExplanation {
    /// No supported plain text. HTML-only mail is the usual case.
    NoPlainText {
        has_html: bool,
    },
    Encrypted,
    /// S/MIME, which this feature neither decrypts nor verifies.
    SecuredWithSMime,
    /// A character set mail-parser does not know.
    UnknownCharset(String),
    /// A Content-Transfer-Encoding no client knows.
    UnknownEncoding(String),
    /// The MIME entity itself could not be read.
    Undecodable,
}

impl ContentExplanation {
    /// Content this version does not show by design, as opposed to content
    /// that could not be read.
    pub fn is_by_design(&self) -> bool {
        matches!(
            self,
            Self::NoPlainText { .. } | Self::Encrypted | Self::SecuredWithSMime
        )
    }
}

/// Chooses the plain-text parts to read, without reading any payload.
pub fn select_text_parts(root: &MimePart) -> TextSelection {
    let mut walk = Walk::default();
    visit(root, &mut walk);
    let selection = match (walk.parts.is_empty(), walk.explanation) {
        (false, _) => TextSelection::Parts(walk.parts),
        (true, Some(explanation)) => TextSelection::Explained(explanation),
        (true, None) => TextSelection::Explained(ContentExplanation::NoPlainText {
            has_html: walk.has_html,
        }),
    };
    match &selection {
        TextSelection::Parts(sections) => tracing::debug!(
            sections = sections
                .iter()
                .map(|section| section_name(section))
                .collect::<Vec<_>>()
                .join(" "),
            "text parts selected"
        ),
        TextSelection::Explained(explanation) => {
            tracing::debug!(?explanation, "no text part selected");
        }
    }
    selection
}

/// A section as IMAP writes it, such as `2.1`; the whole message is empty.
fn section_name(section: &[u32]) -> String {
    section
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

#[derive(Default)]
struct Walk {
    parts: Vec<Vec<u32>>,
    has_html: bool,
    explanation: Option<ContentExplanation>,
}

fn visit(part: &MimePart, walk: &mut Walk) {
    // An attachment and everything inside it is a file, not the message text.
    if part.disposition_is("attachment") {
        return;
    }
    match (part.media_type.as_str(), part.media_subtype.as_str()) {
        ("multipart", "alternative") => {
            let mut chosen = None;
            for (number, child) in (1..).zip(&part.children) {
                let mut branch = Walk::default();
                visit(child, &mut branch);
                walk.has_html |= branch.has_html;
                if branch.parts.is_empty() {
                    walk.explanation = walk.explanation.take().or(branch.explanation);
                } else {
                    // The last branch with plain text is the richest one to show.
                    chosen = Some((number, branch.parts));
                }
            }
            if let Some((number, parts)) = chosen {
                tracing::debug!(
                    section = section_name(&part.section),
                    alternative = number,
                    "the last alternative with plain text was chosen"
                );
                walk.parts.extend(parts);
            }
        }
        // Only the signed content matters; the signature is not verified.
        ("multipart", "signed") => walk_first_child(part, walk),
        ("multipart", "encrypted") => {
            walk.explanation
                .get_or_insert(ContentExplanation::Encrypted);
        }
        // A related set has a root part; the others are resources it uses.
        ("multipart", "related") => walk_related_root(part, walk),
        ("multipart", _) => {
            for child in &part.children {
                visit(child, walk);
            }
        }
        // A file name without an explicit inline disposition marks an attached file.
        ("text", "plain") if !part.has_file_name() || part.disposition_is("inline") => {
            walk.parts.push(part.section.clone());
        }
        ("text", "plain") => tracing::debug!(
            section = section_name(&part.section),
            "text part left out as a file"
        ),
        ("text", "html") => walk.has_html = true,
        ("application", "pkcs7-mime" | "x-pkcs7-mime") => {
            walk.explanation
                .get_or_insert(ContentExplanation::SecuredWithSMime);
        }
        // Nested messages, images and other payloads are not the message text.
        _ => {}
    }
}

/// `name` itself, the extended `name*`, and continuations such as `name*0`
/// and `name*1*`. Parameter names reach this crate in lower case.
fn is_file_name_parameter(parameter: &str) -> bool {
    let Some(suffix) = parameter.strip_prefix("name") else {
        return false;
    };
    let Some(continuation) = suffix.strip_prefix('*') else {
        // The plain `name` parameter, and nothing else that merely starts with it.
        return suffix.is_empty();
    };
    // `name*` carries an extended value; a digit selects one continuation.
    let section = continuation.strip_suffix('*').unwrap_or(continuation);
    continuation.is_empty()
        || (!section.is_empty() && section.bytes().all(|byte| byte.is_ascii_digit()))
}

fn walk_first_child(part: &MimePart, walk: &mut Walk) {
    if let Some(child) = part.children.first() {
        visit(child, walk);
    }
}

/// The root of a related set: the child whose Content-ID the `start`
/// parameter names, or the first child when there is no `start`, the server
/// reported no Content-ID for that child, or nothing matches.
fn walk_related_root(part: &MimePart, walk: &mut Walk) {
    let named_root = part.parameter("start").and_then(|start| {
        let start = start.trim_start_matches('<').trim_end_matches('>');
        part.children
            .iter()
            .find(|child| child.content_id.as_deref() == Some(start))
    });
    tracing::debug!(
        section = section_name(&part.section),
        start_matched = named_root.is_some(),
        "the root of a related set was chosen"
    );
    match named_root {
        Some(root) => visit(root, walk),
        None => walk_first_child(part, walk),
    }
}

/// Decodes one text part from its MIME header and its still-encoded body.
/// Invalid bytes become replacement characters; only an unknown character set
/// or transfer encoding hides the text.
pub fn decode_text_part(mime_header: &[u8], body: &[u8]) -> Result<String, ContentExplanation> {
    let decoded = decode_entity(mime_header, body);
    if let Err(cause) = &decoded {
        tracing::debug!(?cause, "text part could not be decoded");
    }
    decoded
}

fn decode_entity(mime_header: &[u8], body: &[u8]) -> Result<String, ContentExplanation> {
    let mut entity = Vec::with_capacity(mime_header.len() + body.len() + 2);
    entity.extend_from_slice(mime_header);
    // A header the server returned without its line ending needs one.
    if !entity.ends_with(b"\n") {
        entity.extend_from_slice(b"\r\n");
    }
    // The header and the body need one empty line between them.
    if !entity.ends_with(b"\r\n\r\n") && !entity.ends_with(b"\n\n") {
        entity.extend_from_slice(b"\r\n");
    }
    entity.extend_from_slice(body);
    let message = MessageParser::default()
        .parse(&entity)
        .ok_or(ContentExplanation::Undecodable)?;
    let part = message
        .parts
        .first()
        .ok_or(ContentExplanation::Undecodable)?;
    let encoding = part.content_transfer_encoding().unwrap_or_default();
    if !encoding.is_empty() && !is_known_encoding(encoding) {
        return Err(ContentExplanation::UnknownEncoding(encoding.to_owned()));
    }
    // The parser answers content it cannot decode with the still-encoded body,
    // which would otherwise be shown as the message text.
    if part.is_encoding_problem {
        return Err(ContentExplanation::Undecodable);
    }
    // A base64 payload that ends inside a group of four loses its last
    // characters silently, so the text would be short without saying so.
    if encoding.eq_ignore_ascii_case("base64") && !base64_groups_are_complete(body) {
        return Err(ContentExplanation::Undecodable);
    }
    if let Some(charset) = part.content_type().and_then(|ty| ty.attribute("charset"))
        && !is_known_charset(charset)
    {
        return Err(ContentExplanation::UnknownCharset(charset.to_owned()));
    }
    match &part.body {
        // NUL cannot reach GTK text APIs.
        PartType::Text(text) => {
            let text = text.replace('\0', "\u{FFFD}");
            let flowed = flowed_join(part);
            let text = match flowed {
                Some(delete_space) => unflow_text(&text, delete_space),
                None => text,
            };
            tracing::debug!(
                charset = part.content_type().and_then(|ty| ty.attribute("charset")),
                transfer_encoding = (!encoding.is_empty()).then_some(encoding),
                flowed = flowed.is_some(),
                characters_out = text.chars().count(),
                "text part decoded"
            );
            Ok(text)
        }
        _ => Err(ContentExplanation::Undecodable),
    }
}

/// Whether the part is `format=flowed`, and whether `delsp=yes` says the space
/// that marks a soft line break is not part of the text.
fn flowed_join(part: &mail_parser::MessagePart<'_>) -> Option<bool> {
    let content_type = part.content_type()?;
    let format = content_type.attribute("format")?;
    format.eq_ignore_ascii_case("flowed").then(|| {
        content_type
            .attribute("delsp")
            .is_some_and(|delsp| delsp.eq_ignore_ascii_case("yes"))
    })
}

/// Joins the soft line breaks of RFC 3676: a line ending in a space continues
/// in the next line of the same quoting depth. The `-- ` signature separator
/// ends a paragraph, and one space the sender put in front of a line to
/// protect it is not part of the text.
fn unflow_text(text: &str, delete_space: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    // The quoting depth of the paragraph still waiting for its next line.
    let mut open_paragraph: Option<usize> = None;
    for raw_line in text.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let depth = line.bytes().take_while(|byte| *byte == b'>').count();
        let quoted_text = &line[depth..];
        let mut content = quoted_text
            .strip_prefix(' ')
            .unwrap_or(quoted_text)
            .to_owned();
        let ends_paragraph = content == "-- " || !content.ends_with(' ');
        if !ends_paragraph && delete_space {
            content.pop();
        }
        match open_paragraph {
            Some(open) if open == depth => lines
                .last_mut()
                .expect("an open paragraph has its line")
                .push_str(&content),
            _ => lines.push(format!("{}{content}", quote_prefix(depth))),
        }
        open_paragraph = (!ends_paragraph).then_some(depth);
    }
    lines.join("\n")
}

fn quote_prefix(depth: usize) -> String {
    match depth {
        0 => String::new(),
        depth => format!("{} ", ">".repeat(depth)),
    }
}

/// Whether a base64 body ends on a complete group of four characters.
/// Line endings and spaces separate the groups and do not belong to them.
fn base64_groups_are_complete(body: &[u8]) -> bool {
    let characters = body
        .iter()
        .filter(|byte| !byte.is_ascii_whitespace())
        .count();
    characters % 4 == 0
}

fn is_known_encoding(encoding: &str) -> bool {
    matches!(
        encoding.to_ascii_lowercase().as_str(),
        "7bit" | "8bit" | "binary" | "base64" | "quoted-printable"
    )
}

fn is_known_charset(charset: &str) -> bool {
    matches!(
        charset.to_ascii_lowercase().as_str(),
        "utf-8" | "utf8" | "us-ascii" | "ascii"
    ) || charset_decoder(charset.as_bytes()).is_some()
}

/// Decodes the selected parts of one message, each from its MIME header and
/// its still-encoded body, and joins them into the message's text. One
/// unreadable part leaves no complete text to show.
pub fn decode_message_text<'a>(
    parts: impl IntoIterator<Item = (&'a [u8], &'a [u8])>,
) -> Result<String, ContentExplanation> {
    let decoded: Vec<String> = parts
        .into_iter()
        .map(|(mime_header, body)| decode_text_part(mime_header, body))
        .collect::<Result<_, _>>()?;
    Ok(decoded.join("\n\n"))
}

/// Subject, sender and recipients for the list and the reader.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplayFields {
    pub subject: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// Decodes the From, To and Subject header lines of a received message.
/// Missing or unreadable fields are left out; the row stays usable.
pub fn decode_display_fields(header_lines: &[u8]) -> DisplayFields {
    let Some(message) = MessageParser::default().parse_headers(header_lines) else {
        return DisplayFields::default();
    };
    let names = |addresses: Option<&mail_parser::Address<'_>>| {
        display_names(
            addresses?
                .iter()
                .map(|address| (address.name(), address.address())),
        )
    };
    DisplayFields {
        subject: message.subject().map(str::to_owned),
        from: names(message.from()),
        to: names(message.to()),
    }
}

/// How senders or recipients are shown, given each one's name and address:
/// the name, else the address, joined by ", ". `None` when no one has either.
pub fn display_names<'a>(
    names: impl IntoIterator<Item = (Option<&'a str>, Option<&'a str>)>,
) -> Option<String> {
    let names: Vec<&str> = names
        .into_iter()
        .filter_map(|(name, address)| name.or(address))
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}
