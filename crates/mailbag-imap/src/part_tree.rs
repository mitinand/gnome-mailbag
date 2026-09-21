// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use async_imap::imap_proto::{BodyContentCommon, BodyContentSinglePart, BodyStructure};

/// One part of a message as the server's BODYSTRUCTURE describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessagePart {
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
    /// Content-ID without its angle brackets, by which a related set names its
    /// root. BODYSTRUCTURE reports it for single parts only.
    pub content_id: Option<String>,
    /// Parts of a multipart. Nested messages are not expanded.
    pub children: Vec<MessagePart>,
}

impl MessagePart {
    pub(crate) fn from_body_structure(structure: &BodyStructure<'_>) -> Self {
        match structure {
            BodyStructure::Multipart { .. } => project(structure, Vec::new()),
            _ => project(structure, vec![1]),
        }
    }
}

fn project(structure: &BodyStructure<'_>, section: Vec<u32>) -> MessagePart {
    match structure {
        BodyStructure::Multipart { common, bodies, .. } => {
            log_part(common, &section, None);
            let children = (1..)
                .zip(bodies)
                .map(|(number, body)| project(body, [section.as_slice(), &[number]].concat()))
                .collect();
            describe(common, section, children, None)
        }
        BodyStructure::Basic { common, other, .. }
        | BodyStructure::Text { common, other, .. }
        | BodyStructure::Message { common, other, .. } => {
            log_part(common, &section, Some(other));
            describe(common, section, Vec::new(), other.id.as_deref())
        }
    }
}

/// One part of the tree at debug, inside the message's span. File names,
/// other parameters, the part's description, its content identifier and an
/// attached message's envelope are never written (specs/003-logging FR-011).
fn log_part(
    common: &BodyContentCommon<'_>,
    section: &[u32],
    single_part: Option<&BodyContentSinglePart<'_>>,
) {
    let parameter = |name: &str| {
        common
            .ty
            .params
            .iter()
            .flatten()
            .find(|(parameter, _)| parameter.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_ref())
    };
    tracing::debug!(
        section = section
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("."),
        content_type = format!("{}/{}", common.ty.ty, common.ty.subtype),
        charset = parameter("charset"),
        format = parameter("format"),
        delsp = parameter("delsp"),
        disposition = common
            .disposition
            .as_ref()
            .map(|disposition| disposition.ty.as_ref()),
        transfer_encoding = single_part.map(|part| tracing::field::debug(&part.transfer_encoding)),
        size = single_part.map(|part| part.octets),
        "message part"
    );
}

/// A Content-ID is written `<name@host>` in the header and reported that way;
/// the `start` parameter of a related set may carry it with or without the
/// brackets.
fn strip_angle_brackets(content_id: &str) -> String {
    content_id
        .strip_prefix('<')
        .and_then(|id| id.strip_suffix('>'))
        .unwrap_or(content_id)
        .to_owned()
}

fn describe(
    common: &BodyContentCommon<'_>,
    section: Vec<u32>,
    children: Vec<MessagePart>,
    content_id: Option<&str>,
) -> MessagePart {
    MessagePart {
        section,
        media_type: common.ty.ty.to_ascii_lowercase(),
        media_subtype: common.ty.subtype.to_ascii_lowercase(),
        parameters: common
            .ty
            .params
            .iter()
            .flatten()
            .map(|(name, value)| (name.to_ascii_lowercase(), value.to_string()))
            .collect(),
        disposition: common
            .disposition
            .as_ref()
            .map(|disposition| disposition.ty.to_ascii_lowercase()),
        content_id: content_id.map(strip_angle_brackets),
        children,
    }
}
