// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::tests::{load_sample, multipart, part};

/// A single part as the protocol layer delivers it, split at its first empty
/// line into the MIME header and the still-encoded body.
fn split_part(entity: &[u8]) -> (&[u8], &[u8]) {
    let blank = entity
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the part has a header");
    (&entity[..blank + 4], &entity[blank + 4..])
}

fn preview_of_part(entity: &[u8], is_html: bool) -> String {
    let (header, body) = split_part(entity);
    preview_of_piece(header, body, is_html)
}

/// The preview of a sample's chosen part, read whole.
fn preview_of_sample(name: &str) -> String {
    let sample = load_sample(name);
    let chosen = select_preview_part(&sample.root).expect("the sample has a preview part");
    let (header, body) = &sample.sections[&section_name(&chosen.section)];
    preview_of_piece(header, body, chosen.is_html)
}

fn html_part(section: &[u32]) -> MimePart {
    part(section, "text", "html")
}

fn with_file_name(part: MimePart, disposition: Option<&str>) -> MimePart {
    MimePart {
        parameters: vec![("name".to_owned(), "page.html".to_owned())],
        disposition: disposition.map(str::to_owned),
        ..part
    }
}

#[test]
fn the_page_is_chosen_before_the_plain_text() {
    let preview_part = |section: &[u32], is_html| {
        Some(PreviewPart {
            section: section.to_vec(),
            is_html,
        })
    };
    let cases = [
        ("01-plain-utf8.eml", preview_part(&[1], false)),
        ("04-alternative.eml", preview_part(&[2], true)),
        ("05-html-only.eml", preview_part(&[1], true)),
        // The attached text file is not the message text.
        ("06-attachment-text.eml", preview_part(&[1], false)),
        // The page inside the related set's root.
        ("14-related.eml", preview_part(&[1, 2], true)),
        // The nested message is skipped entirely.
        ("17-nested-message.eml", preview_part(&[1], false)),
        ("12-encrypted.eml", None),
        ("13-smime-opaque.eml", None),
    ];
    for (name, expected) in cases {
        let sample = load_sample(name);
        assert_eq!(select_preview_part(&sample.root), expected, "{name}");
    }
}

#[test]
fn an_attached_page_is_never_the_preview() {
    let plain_after = |page| multipart("mixed", vec![page, part(&[2], "text", "plain")]);
    for page in [
        with_file_name(html_part(&[1]), Some("attachment")),
        with_file_name(html_part(&[1]), None),
        MimePart {
            disposition: Some("attachment".to_owned()),
            ..html_part(&[1])
        },
    ] {
        let message = plain_after(page);
        assert_eq!(
            select_preview_part(&message),
            Some(PreviewPart {
                section: vec![2],
                is_html: false,
            }),
            "{message:?}"
        );
    }
    // A page with a file name that is marked inline is the message's page.
    let message = plain_after(with_file_name(html_part(&[1]), Some("inline")));
    assert_eq!(
        select_preview_part(&message).map(|chosen| chosen.section),
        Some(vec![1])
    );
}

#[test]
fn samples_preview_their_first_readable_words() {
    let cases = [
        ("01-plain-utf8.eml", "Обычный текст в UTF-8."),
        ("02-cp1251-base64.eml", "Текст в кодировке Windows-1251."),
        (
            "03-koi8r-qp.eml",
            "Текст в KOI8-R, закодированный quoted-printable.",
        ),
        ("04-alternative.eml", "HTML версия"),
        ("05-html-only.eml", "Только HTML"),
        ("06-attachment-text.eml", "Основной текст письма."),
        ("17-nested-message.eml", "Пересылаю письмо."),
    ];
    for (name, beginning) in cases {
        let preview = preview_of_sample(name);
        assert!(preview.starts_with(beginning), "{name}: {preview:?}");
    }
}

#[test]
fn a_western_character_set_decodes() {
    let part = b"Content-Type: text/plain; charset=iso-8859-1\r\n\
        Content-Transfer-Encoding: 8bit\r\n\r\nCaf\xe9 cr\xe8me.";
    assert_eq!(preview_of_part(part, false), "Café crème.");
}

#[test]
fn an_unknown_character_set_gives_no_preview() {
    let part = b"Content-Type: text/plain; charset=x-unknown\r\n\r\nWords.";
    assert_eq!(preview_of_part(part, false), "");
}

const CUT_TEXT: &str = "Привет! Это проверка превью: строка, которая длиннее одной группы.";

/// Every cut of a piece previews a beginning of the whole text, without a
/// replacement mark and without the undecodable piece hiding everything.
fn assert_every_cut_is_a_beginning(header: &str, body: &str) {
    let whole = preview_of_piece(header.as_bytes(), body.as_bytes(), false);
    assert_eq!(whole, CUT_TEXT);
    for length in 0..body.len() {
        let preview = preview_of_piece(header.as_bytes(), &body.as_bytes()[..length], false);
        assert!(
            whole.starts_with(&preview) && !preview.contains('\u{FFFD}'),
            "cut at {length}: {preview:?}"
        );
        if length > body.len() / 2 {
            assert!(!preview.is_empty(), "cut at {length} lost the text");
        }
    }
}

#[test]
fn a_base64_piece_cut_inside_a_group_or_a_character_keeps_its_beginning() {
    assert_every_cut_is_a_beginning(
        "Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n",
        "0J/RgNC40LLQtdGCISDQrdGC0L4g0L/RgNC+0LLQtdGA0LrQsCDQv9GA0LXQstGM0Y46INGB0YLR\r\n\
         gNC+0LrQsCwg0LrQvtGC0L7RgNCw0Y8g0LTQu9C40L3QvdC10LUg0L7QtNC90L7QuSDQs9GA0YPQ\r\n\
         v9C/0Ysu\r\n",
    );
}

#[test]
fn a_quoted_printable_piece_cut_inside_an_escape_keeps_its_beginning() {
    assert_every_cut_is_a_beginning(
        "Content-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\r\n",
        "=D0=9F=D1=80=D0=B8=D0=B2=D0=B5=D1=82! =D0=AD=D1=82=D0=BE =D0=BF=D1=80=D0=BE=\r\n\
         =D0=B2=D0=B5=D1=80=D0=BA=D0=B0 =D0=BF=D1=80=D0=B5=D0=B2=D1=8C=D1=8E: =D1=81=\r\n\
         =D1=82=D1=80=D0=BE=D0=BA=D0=B0, =D0=BA=D0=BE=D1=82=D0=BE=D1=80=D0=B0=D1=8F =\r\n\
         =D0=B4=D0=BB=D0=B8=D0=BD=D0=BD=D0=B5=D0=B5 =D0=BE=D0=B4=D0=BD=D0=BE=D0=B9 =\r\n\
         =D0=B3=D1=80=D1=83=D0=BF=D0=BF=D1=8B.",
    );
}

#[test]
fn an_eight_bit_piece_cut_inside_a_character_keeps_its_beginning() {
    assert_every_cut_is_a_beginning(
        "Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
        CUT_TEXT,
    );
}

const NEWSLETTER: &str = "Content-Type: text/html; charset=utf-8\r\n\r\n\
    <!DOCTYPE html><html><head><title>Page title</title>\
    <style>.hidden { display: none; } p { color: red; }</style></head>\
    <body><div class=\"hidden\" style=\"display:none\">Hidden opening line</div>\
    <script>document.write('script text');</script>\
    <!-- a comment -->\
    <img src=\"https://example.com/logo.png\" alt=\"Logo text\">\
    <h1>Weekly news</h1><p>First block.</p><p>Second&nbsp;block with \
    <a href=\"https://example.com/more\">the link's words</a>.</p>\
    <ul><li>Item one</li><li>Item two</li></ul><table><tr><td>Cell one</td>\
    <td>Cell two</td></tr></table></body></html>";

#[test]
fn a_page_gives_its_visible_words_only() {
    let preview = preview_of_part(NEWSLETTER.as_bytes(), true);
    assert_eq!(
        preview,
        "Hidden opening line Weekly news First block. Second block with the link's \
         words. Item one Item two Cell one Cell two"
    );
    for left_out in [
        "<",
        "Page title",
        "color",
        "script",
        "comment",
        "Logo",
        "example.com",
    ] {
        assert!(!preview.contains(left_out), "{left_out}: {preview:?}");
    }
}

#[test]
fn a_page_of_images_alone_gives_no_preview() {
    let part = "Content-Type: text/html; charset=utf-8\r\n\r\n\
        <html><body><a href=\"https://example.com\"><img src=\"cid:banner\" alt=\"Banner\"></a>\
        </body></html>";
    assert_eq!(preview_of_part(part.as_bytes(), true), "");
}

#[test]
fn a_page_cut_inside_its_head_gives_no_preview() {
    let (header, body) = split_part(NEWSLETTER.as_bytes());
    let inside_style = NEWSLETTER.find("color").unwrap() - header.len();
    assert_eq!(preview_of_piece(header, &body[..inside_style], true), "");
}

#[test]
fn white_space_and_invisible_characters_are_normalised() {
    let text = "\u{FEFF}  First\tline\r\n\r\nsecond\u{2800}line\u{180E}and \
        in\u{00AD}vis\u{200B}i\u{2060}ble\u{202E} marks \u{7}\n";
    assert_eq!(
        preview_of_text(text),
        "First line second line and invisible marks"
    );
}

#[test]
fn accents_and_brackets_stay() {
    // Vietnamese written with combining marks, as some senders decompose it.
    let text = "Tie\u{0302}\u{0301}ng Vie\u{0323}\u{0302}t [Ticket 4711]";
    assert_eq!(preview_of_text(text), text);
}

#[test]
fn a_preview_is_cut_at_its_first_four_hundred_characters() {
    let preview = preview_of_text(&"слово ".repeat(100));
    assert_eq!(preview.chars().count(), 400);
    // A space that is the last character to fit is not kept.
    let preview = preview_of_text(&"abcd ".repeat(100));
    assert_eq!(preview.chars().count(), 399);
    let preview = preview_of_text(&"я".repeat(500));
    assert_eq!(preview, "я".repeat(400));
}

#[test]
fn a_services_preview_is_normalised() {
    assert_eq!(
        preview_of_text("Hello,\r\n\r\nThe meeting moves to Friday.\u{200B}\r\n"),
        "Hello, The meeting moves to Friday."
    );
}
