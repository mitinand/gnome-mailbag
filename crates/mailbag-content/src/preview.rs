// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview a list row shows: the first words of a message's text, made
//! from the beginning of its web-page or plain-text part.

use mail_parser::{MessageParser, MimeHeaders, decoders::html::html_to_text};

use crate::{MimePart, decode_text_part, section_name, walk_message};

#[cfg(test)]
mod tests;

/// How much of the chosen part is read for a preview.
pub const PREVIEW_PIECE_BYTES: u32 = 16_384;

/// The longest preview kept, in characters; the row's two lines cut it further.
const PREVIEW_CHARACTERS: usize = 400;

/// The part a preview is made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewPart {
    /// IMAP section numbers, as in [`MimePart::section`].
    pub section: Vec<u32>,
    /// Whether the part is a web page rather than plain text.
    pub is_html: bool,
}

/// Chooses the part to read for a preview: the first web page that is not an
/// attached file, else the first plain-text part the reader would read;
/// `None` for encrypted and S/MIME-secured messages and messages without text.
pub fn select_preview_part(root: &MimePart) -> Option<PreviewPart> {
    let walk = walk_message(root);
    let chosen = match (walk.html_part, walk.parts.into_iter().next()) {
        (Some(section), _) => Some(PreviewPart {
            section,
            is_html: true,
        }),
        (None, Some(section)) => Some(PreviewPart {
            section,
            is_html: false,
        }),
        (None, None) => None,
    };
    match &chosen {
        Some(part) => tracing::debug!(
            section = section_name(&part.section),
            is_html = part.is_html,
            "preview part selected"
        ),
        None => tracing::debug!("no preview part selected"),
    }
    chosen
}

/// Makes a preview from the beginning of a part: its MIME header and the
/// first bytes of its still-encoded body. Empty when nothing readable comes
/// out, including a character set or encoding the reader cannot decode.
pub fn preview_of_piece(mime_header: &[u8], piece: &[u8], is_html: bool) -> String {
    let piece = clean_cut(piece, transfer_encoding(mime_header).as_deref());
    let Ok(text) = decode_text_part(mime_header, piece) else {
        return String::new();
    };
    // A multi-byte character cut at the piece's end decodes to one mark.
    let text = text.strip_suffix('\u{FFFD}').unwrap_or(&text);
    let preview = match is_html {
        true => normalise_words(&page_words(text)),
        false => normalise_words(text),
    };
    tracing::debug!(
        is_html,
        bytes_in = piece.len(),
        characters_out = preview.chars().count(),
        "preview made"
    );
    preview
}

/// Makes a preview from a text the service already provides.
pub fn preview_of_text(text: &str) -> String {
    normalise_words(text)
}

fn transfer_encoding(mime_header: &[u8]) -> Option<String> {
    let header = MessageParser::default().parse_headers(mime_header)?;
    let part = header.parts.first()?;
    part.content_transfer_encoding()
        .map(str::to_ascii_lowercase)
}

/// Drops a base64 group the cut left with fewer than four characters, which
/// the reader's decoder would refuse. A quoted-printable escape cut short
/// needs nothing: the parser leaves it out itself.
fn clean_cut<'a>(piece: &'a [u8], transfer_encoding: Option<&str>) -> &'a [u8] {
    match transfer_encoding {
        Some("base64") => {
            let mut incomplete = piece
                .iter()
                .filter(|byte| !byte.is_ascii_whitespace())
                .count()
                % 4;
            let mut end = piece.len();
            while incomplete > 0 {
                end -= 1;
                if !piece[end].is_ascii_whitespace() {
                    incomplete -= 1;
                }
            }
            &piece[..end]
        }
        _ => piece,
    }
}

/// Tags whose content starts a new block of the page, so that adjoining
/// blocks' words stay apart once the markup is gone.
const BLOCK_TAGS: &str = "p div br li tr td th h1 h2 h3 h4 h5 h6 table ul ol \
    blockquote pre hr section article header footer dd dt";

/// The visible words of a web page: a space before every opening or closing
/// block tag, then the page turned into text.
fn page_words(html: &str) -> String {
    let mut spaced = String::with_capacity(html.len() + html.len() / 16);
    for (position, character) in html.char_indices() {
        if character == '<' && starts_block_tag(&html[position + 1..]) {
            spaced.push(' ');
        }
        spaced.push(character);
    }
    html_to_text(&spaced)
}

/// Whether the text after a `<` names a block tag, opening or closing.
fn starts_block_tag(after_bracket: &str) -> bool {
    let tag = after_bracket.strip_prefix('/').unwrap_or(after_bracket);
    let name_length = tag.bytes().take_while(u8::is_ascii_alphanumeric).count();
    BLOCK_TAGS
        .split_whitespace()
        .any(|block| tag[..name_length].eq_ignore_ascii_case(block))
}

/// White space becomes one space per run, invisible control and formatting
/// characters go, combining marks stay, and the result is trimmed and cut to
/// its first characters.
fn normalise_words(text: &str) -> String {
    let mut words = String::new();
    let mut characters = 0;
    let mut space_pending = false;
    for character in text.chars() {
        if is_blank(character) {
            space_pending = !words.is_empty();
            continue;
        }
        if is_invisible(character) {
            continue;
        }
        if space_pending {
            words.push(' ');
            characters += 1;
            space_pending = false;
        }
        if characters >= PREVIEW_CHARACTERS {
            break;
        }
        words.push(character);
        characters += 1;
    }
    // A space may have been the last character that fitted.
    words.trim_end().to_owned()
}

/// White space, and the two characters that look blank without being white
/// space: the braille blank and the Mongolian vowel separator.
fn is_blank(character: char) -> bool {
    character.is_whitespace() || matches!(character, '\u{2800}' | '\u{180E}')
}

fn is_invisible(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00AD}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206F}'
                | '\u{FEFF}'
        )
}
