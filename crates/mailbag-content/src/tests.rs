// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use mail_parser::Message;
use std::{collections::BTreeMap, path::PathBuf};

/// A sample message as the protocol layer delivers it: the structure the
/// server would report, and the raw bytes of each section.
struct Sample {
    root: MimePart,
    /// Section name, such as `2.1`, to its MIME header and encoded body.
    sections: BTreeMap<String, (Vec<u8>, Vec<u8>)>,
    /// The From, To and Subject lines, as BODY[HEADER.FIELDS] returns them.
    header_lines: Vec<u8>,
}

impl Sample {
    /// The text of the selected parts, joined as the reader shows it.
    fn text(&self, selection: &TextSelection) -> Result<String, ContentExplanation> {
        let TextSelection::Parts(parts) = selection else {
            panic!("the sample has no text parts: {selection:?}");
        };
        let decoded: Result<Vec<String>, ContentExplanation> = parts
            .iter()
            .map(|part| {
                let (header, body) = &self.sections[&section_name(part)];
                decode_text_part(header, body)
            })
            .collect();
        Ok(join_message_text(&decoded?))
    }
}

fn section_name(section: &[u32]) -> String {
    section
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn load_sample(name: &str) -> Sample {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/mime")
        .join(name);
    let raw = std::fs::read(&path).unwrap_or_else(|error| panic!("read {name}: {error}"));
    let message = MessageParser::default()
        .parse(&raw)
        .unwrap_or_else(|| panic!("{name} parses"));
    let mut sections = BTreeMap::new();
    let root_section = match message.parts[0].body {
        PartType::Multipart(_) => Vec::new(),
        _ => vec![1],
    };
    let root = describe(&message, 0, root_section, &raw, &mut sections);
    Sample {
        root,
        sections,
        header_lines: raw[..message.parts[0].offset_body as usize].to_vec(),
    }
}

/// Describes one part as a server's BODYSTRUCTURE would, and keeps its bytes.
fn describe(
    message: &Message<'_>,
    id: usize,
    section: Vec<u32>,
    raw: &[u8],
    sections: &mut BTreeMap<String, (Vec<u8>, Vec<u8>)>,
) -> MimePart {
    let part = &message.parts[id];
    let children = match &part.body {
        PartType::Multipart(ids) => (1..)
            .zip(ids)
            .map(|(number, child)| {
                let child_section = [section.as_slice(), &[number]].concat();
                describe(message, *child as usize, child_section, raw, sections)
            })
            .collect(),
        _ => Vec::new(),
    };
    if !section.is_empty() {
        let header = raw[part.offset_header as usize..part.offset_body as usize].to_vec();
        let body = raw[part.offset_body as usize..part.offset_end as usize].to_vec();
        sections.insert(section_name(&section), (header, body));
    }
    let content_type = part.content_type();
    MimePart {
        section,
        media_type: content_type.map_or("text".to_owned(), |ty| ty.c_type.to_ascii_lowercase()),
        media_subtype: content_type
            .and_then(|ty| ty.c_subtype.as_deref())
            .unwrap_or("plain")
            .to_ascii_lowercase(),
        parameters: content_type
            .and_then(|ty| ty.attributes.as_deref())
            .unwrap_or_default()
            .iter()
            .map(|attribute| {
                (
                    attribute.name.to_ascii_lowercase(),
                    attribute.value.to_string(),
                )
            })
            .collect(),
        disposition: part
            .content_disposition()
            .map(|disposition| disposition.c_type.to_ascii_lowercase()),
        content_id: part
            .content_id()
            .map(|id| id.trim_start_matches('<').trim_end_matches('>').to_owned()),
        children,
    }
}

fn parts(sections: &[&[u32]]) -> TextSelection {
    TextSelection::Parts(sections.iter().map(|section| section.to_vec()).collect())
}

#[test]
fn samples_select_the_plain_text_to_read() {
    let cases = [
        ("01-plain-utf8.eml", parts(&[&[1]])),
        ("02-cp1251-base64.eml", parts(&[&[1]])),
        ("03-koi8r-qp.eml", parts(&[&[1]])),
        ("15-iso2022jp.eml", parts(&[&[1]])),
        // The last branch with plain text, without downloading the HTML one.
        ("04-alternative.eml", parts(&[&[1]])),
        (
            "05-html-only.eml",
            TextSelection::Explained(ContentExplanation::NoPlainText { has_html: true }),
        ),
        // The attached text file is not the message text.
        ("06-attachment-text.eml", parts(&[&[1]])),
        // Only the signed content; the signature is never fetched.
        ("07-signed.eml", parts(&[&[1]])),
        // Both inline parts of a mixed message, including the list footer.
        ("11-mailman-footer.eml", parts(&[&[1], &[2]])),
        (
            "12-encrypted.eml",
            TextSelection::Explained(ContentExplanation::Encrypted),
        ),
        (
            "13-smime-opaque.eml",
            TextSelection::Explained(ContentExplanation::SecuredWithSMime),
        ),
        // The first child of a related set, whose plain branch is section 1.1.
        ("14-related.eml", parts(&[&[1, 1]])),
        // The child the start parameter names, not the first one.
        ("20-related-start.eml", parts(&[&[2]])),
        // The nested message is skipped entirely.
        ("17-nested-message.eml", parts(&[&[1]])),
    ];
    for (name, expected) in cases {
        let sample = load_sample(name);
        assert_eq!(select_text_parts(&sample.root), expected, "{name}");
    }
}

#[test]
fn selected_parts_decode_to_their_text() {
    let cases = [
        ("01-plain-utf8.eml", "Обычный текст в UTF-8."),
        ("02-cp1251-base64.eml", "Текст в кодировке Windows-1251."),
        (
            "03-koi8r-qp.eml",
            "Текст в KOI8-R, закодированный quoted-printable.",
        ),
        ("15-iso2022jp.eml", "こんにちは。日本語のテキストです。"),
        ("04-alternative.eml", "Текстовая версия."),
        ("06-attachment-text.eml", "Основной текст письма."),
        ("07-signed.eml", "Подписанный текст."),
        ("14-related.eml", "Текст внутри related."),
        ("20-related-start.eml", "Текст, на который указывает start."),
        ("17-nested-message.eml", "Пересылаю письмо."),
    ];
    for (name, expected) in cases {
        let sample = load_sample(name);
        let text = sample
            .text(&select_text_parts(&sample.root))
            .unwrap_or_else(|explanation| panic!("{name}: {explanation:?}"));
        assert!(text.contains(expected), "{name}: {text:?}");
        assert!(
            !text.contains("Содержимое вложения") && !text.contains("вложенного письма"),
            "{name} must not include excluded parts: {text:?}"
        );
    }
}

#[test]
fn flowed_text_becomes_whole_paragraphs_again() {
    let sample = load_sample("18-flowed.eml");
    let text = sample.text(&select_text_parts(&sample.root)).unwrap();
    assert!(
        text.contains("Это абзац, который отправитель перенёс по границе узкого окна, и он должен снова стать одной строкой."),
        "{text:?}"
    );
    // Quoting depth keeps its own paragraph.
    assert!(
        text.contains("> Цитата тоже переносится мягко."),
        "{text:?}"
    );
    // A soft break that meets another quoting depth ends its paragraph. The
    // space that marked the break belongs to the line, so it stays.
    assert!(
        text.contains("> Цитата обрывается на переносе \nОтвет на другой глубине."),
        "{text:?}"
    );
    assert!(text.contains("Обычная строка без переноса."), "{text:?}");
    // The signature separator ends a paragraph instead of joining it.
    assert!(text.contains("-- \nПодпись"), "{text:?}");
}

#[test]
fn a_flowed_message_with_delsp_joins_words_without_a_space() {
    let sample = load_sample("19-flowed-delsp.eml");
    let text = sample.text(&select_text_parts(&sample.root)).unwrap();
    assert!(
        text.contains("interoperability без лишнего пробела."),
        "{text:?}"
    );
}

#[test]
fn text_that_is_not_flowed_keeps_its_line_breaks() {
    let sample = load_sample("11-mailman-footer.eml");
    let text = sample.text(&select_text_parts(&sample.root)).unwrap();
    assert!(text.contains('\n'), "{text:?}");
}

#[test]
fn several_inline_parts_are_joined_with_a_blank_line() {
    let sample = load_sample("11-mailman-footer.eml");
    let text = sample.text(&select_text_parts(&sample.root)).unwrap();
    assert!(text.contains("Сообщение списка."), "{text:?}");
    assert!(text.contains("Подвал списка рассылки."), "{text:?}");
    assert!(text.contains("\n\n"), "{text:?}");
}

#[test]
fn invalid_bytes_become_replacement_characters() {
    for name in ["08-broken-utf8.eml", "09-no-charset-8bit.eml"] {
        let sample = load_sample(name);
        let text = sample.text(&select_text_parts(&sample.root)).unwrap();
        assert!(text.contains('\u{FFFD}'), "{name}: {text:?}");
        // The rest of the body is still shown.
        assert!(text.len() > 3, "{name}: {text:?}");
    }
}

#[test]
fn an_unknown_charset_or_encoding_explains_the_missing_text() {
    let cases = [
        (
            "10-unknown-charset.eml",
            ContentExplanation::UnknownCharset("x-mailbag-unknown".to_owned()),
        ),
        (
            "16-unknown-encoding.eml",
            ContentExplanation::UnknownEncoding("x-mailbag-unknown".to_owned()),
        ),
    ];
    for (name, expected) in cases {
        let sample = load_sample(name);
        let selection = select_text_parts(&sample.root);
        assert_eq!(selection, parts(&[&[1]]), "{name}");
        assert_eq!(sample.text(&selection), Err(expected), "{name}");
    }
}

/// The same text as valid base64, in groups of four characters.
const BASE64_HELLO: &str = "SGVsbG8gd29ybGQh";

fn decode_base64(body: &str) -> Result<String, ContentExplanation> {
    decode_text_part(
        b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n",
        body.as_bytes(),
    )
}

#[test]
fn a_payload_the_parser_cannot_decode_is_explained_not_shown() {
    // A stray character makes mail-parser answer with the encoded body itself.
    // The replacement keeps whole groups of four, so only the parser's mark
    // tells that the content was not decoded.
    let replaced = format!("!{}", &BASE64_HELLO[1..]);
    for body in [
        replaced,
        format!("{BASE64_HELLO}!"),
        format!("{BASE64_HELLO}!!!!"),
    ] {
        assert_eq!(
            decode_base64(&body),
            Err(ContentExplanation::Undecodable),
            "{body}"
        );
    }
}

#[test]
fn a_base64_body_cut_short_is_explained_instead_of_shortened() {
    // Two characters short of a group: the parser would drop them silently.
    assert_eq!(
        decode_base64(&BASE64_HELLO[..BASE64_HELLO.len() - 2]),
        Err(ContentExplanation::Undecodable)
    );
    assert_eq!(decode_base64(BASE64_HELLO).as_deref(), Ok("Hello world!"));
}

#[test]
fn base64_folded_into_lines_still_decodes() {
    let folded = format!("{}\r\n {}\r\n", &BASE64_HELLO[..8], &BASE64_HELLO[8..]);
    assert_eq!(decode_base64(&folded).as_deref(), Ok("Hello world!"));
}

#[test]
fn a_nul_byte_never_reaches_the_reader() {
    let text = decode_text_part(
        b"Content-Type: text/plain; charset=utf-8\r\n\r\n",
        b"before\0after",
    );
    assert_eq!(text.as_deref(), Ok("before\u{FFFD}after"));
}

#[test]
fn display_fields_decode_encoded_headers() {
    let sample = load_sample("17-nested-message.eml");
    assert_eq!(
        decode_display_fields(&sample.header_lines),
        DisplayFields {
            subject: Some("Пересылка".to_owned()),
            from: Some("Тестовый отправитель".to_owned()),
            to: Some("Получатель".to_owned()),
        }
    );
}

#[test]
fn missing_display_fields_leave_the_row_usable() {
    let fields = decode_display_fields(b"Subject: \r\n");
    assert_eq!(fields, DisplayFields::default());
}

#[test]
fn nobody_to_name_gives_no_display_names() {
    assert_eq!(display_names([]), None);
    assert_eq!(display_names([(None, None)]), None);
}

/// Builds a part the way a server's BODYSTRUCTURE describes one.
fn part(section: &[u32], media_type: &str, subtype: &str) -> MimePart {
    MimePart {
        section: section.to_vec(),
        media_type: media_type.to_owned(),
        media_subtype: subtype.to_owned(),
        parameters: Vec::new(),
        disposition: None,
        content_id: None,
        children: Vec::new(),
    }
}

fn multipart(subtype: &str, children: Vec<MimePart>) -> MimePart {
    MimePart {
        children,
        ..part(&[], "multipart", subtype)
    }
}

#[test]
fn the_last_alternative_branch_with_plain_text_wins() {
    let message = multipart(
        "alternative",
        vec![
            part(&[1], "text", "plain"),
            part(&[2], "text", "html"),
            part(&[3], "text", "plain"),
        ],
    );
    assert_eq!(select_text_parts(&message), parts(&[&[3]]));
}

#[test]
fn a_named_text_part_counts_as_a_file_unless_it_is_inline() {
    let named = MimePart {
        parameters: vec![("name".to_owned(), "notes.txt".to_owned())],
        ..part(&[2], "text", "plain")
    };
    let inline_named = MimePart {
        disposition: Some("inline".to_owned()),
        ..named.clone()
    };
    let message = |second: MimePart| multipart("mixed", vec![part(&[1], "text", "plain"), second]);
    assert_eq!(select_text_parts(&message(named)), parts(&[&[1]]));
    assert_eq!(
        select_text_parts(&message(inline_named)),
        parts(&[&[1], &[2]])
    );
}

#[test]
fn a_file_name_the_server_left_unfolded_still_marks_an_attachment() {
    // The forms RFC 2231 allows, as a server may leave them in BODYSTRUCTURE.
    for parameters in [
        vec![("name*".to_owned(), "utf-8''notes.txt".to_owned())],
        vec![("name*0*".to_owned(), "utf-8''long".to_owned())],
        vec![
            ("name*0".to_owned(), "long".to_owned()),
            ("name*1*".to_owned(), "name.txt".to_owned()),
        ],
    ] {
        let attached = MimePart {
            parameters: parameters.clone(),
            ..part(&[2], "text", "plain")
        };
        let message = multipart("mixed", vec![part(&[1], "text", "plain"), attached]);
        assert_eq!(
            select_text_parts(&message),
            parts(&[&[1]]),
            "{parameters:?}"
        );
    }
}

#[test]
fn a_parameter_that_only_starts_like_a_file_name_is_not_one() {
    for parameter in ["names", "nameless", "charset", "namex*"] {
        let text = MimePart {
            parameters: vec![(parameter.to_owned(), "value".to_owned())],
            ..part(&[2], "text", "plain")
        };
        let message = multipart("mixed", vec![part(&[1], "text", "plain"), text]);
        assert_eq!(
            select_text_parts(&message),
            parts(&[&[1], &[2]]),
            "{parameter}"
        );
    }
}

/// Builds a related set whose parts carry Content-IDs.
fn related_set(start: Option<&str>) -> MimePart {
    let resource = MimePart {
        content_id: Some("image@example.invalid".to_owned()),
        ..part(&[1], "image", "png")
    };
    let text = MimePart {
        content_id: Some("text@example.invalid".to_owned()),
        ..part(&[2], "text", "plain")
    };
    MimePart {
        parameters: start
            .map(|start| vec![("start".to_owned(), start.to_owned())])
            .unwrap_or_default(),
        children: vec![resource, text],
        ..part(&[], "multipart", "related")
    }
}

#[test]
fn a_related_set_reads_the_root_its_start_parameter_names() {
    // With and without the angle brackets the header uses.
    for start in ["<text@example.invalid>", "text@example.invalid"] {
        assert_eq!(
            select_text_parts(&related_set(Some(start))),
            parts(&[&[2]]),
            "{start}"
        );
    }
}

#[test]
fn a_related_set_without_a_usable_start_reads_its_first_child() {
    for start in [None, Some("<missing@example.invalid>")] {
        assert_eq!(
            select_text_parts(&related_set(start)),
            TextSelection::Explained(ContentExplanation::NoPlainText { has_html: false }),
            "{start:?}"
        );
    }
}

#[test]
fn signed_and_related_messages_use_only_their_first_part() {
    for subtype in ["signed", "related"] {
        let message = multipart(
            subtype,
            vec![part(&[1], "text", "plain"), part(&[2], "text", "plain")],
        );
        assert_eq!(select_text_parts(&message), parts(&[&[1]]), "{subtype}");
    }
}

#[test]
fn an_attachment_hides_the_text_inside_it() {
    let attached_message = MimePart {
        disposition: Some("attachment".to_owned()),
        children: vec![part(&[2, 1], "text", "plain")],
        ..part(&[2], "multipart", "mixed")
    };
    let message = multipart("mixed", vec![part(&[1], "text", "plain"), attached_message]);
    assert_eq!(select_text_parts(&message), parts(&[&[1]]));
}

#[test]
fn a_header_without_its_line_ending_still_separates_the_body() {
    let text = decode_text_part(b"Content-Type: text/plain; charset=utf-8", b"the body");
    assert_eq!(text.as_deref(), Ok("the body"));
}
