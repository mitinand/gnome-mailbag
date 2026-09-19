// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use async_imap::imap_proto::{BodyContentCommon, BodyStructure};

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
            let children = (1..)
                .zip(bodies)
                .map(|(number, body)| project(body, [section.as_slice(), &[number]].concat()))
                .collect();
            describe(common, section, children)
        }
        BodyStructure::Basic { common, .. }
        | BodyStructure::Text { common, .. }
        | BodyStructure::Message { common, .. } => describe(common, section, Vec::new()),
    }
}

fn describe(
    common: &BodyContentCommon<'_>,
    section: Vec<u32>,
    children: Vec<MessagePart>,
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
        children,
    }
}
