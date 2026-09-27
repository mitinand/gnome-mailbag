// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mailbox names in modified UTF-7 (RFC 3501 §5.1.3), the form a server uses
//! unless UTF-8 names are enabled (RFC 6855). Printable ASCII stands for
//! itself, `&-` is `&`, and `&…-` holds UTF-16 in base64 with `,` for `/`
//! and no padding.

/// The name as readable text. A name that is not valid modified UTF-7 is
/// returned as the server sent it, so that it can still be shown.
pub fn decode(name: &str) -> String {
    decode_groups(name).unwrap_or_else(|| name.to_owned())
}

fn decode_groups(name: &str) -> Option<String> {
    let mut decoded = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let group_length = rest[start + 1..].find('-')?;
        let group = &rest[start + 1..start + 1 + group_length];
        if group.is_empty() {
            decoded.push('&');
        } else {
            decoded.push_str(&decode_utf16_group(group)?);
        }
        rest = &rest[start + group_length + 2..];
    }
    decoded.push_str(rest);
    Some(decoded)
}

/// The text of one group between `&` and `-`.
fn decode_utf16_group(group: &str) -> Option<String> {
    let mut code_units = Vec::new();
    let mut bits = 0_u32;
    let mut bit_count = 0;
    for character in group.bytes() {
        let value = match character {
            b'A'..=b'Z' => character - b'A',
            b'a'..=b'z' => character - b'a' + 26,
            b'0'..=b'9' => character - b'0' + 52,
            b'+' => 62,
            b',' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        bit_count += 6;
        if bit_count >= 16 {
            bit_count -= 16;
            code_units.push((bits >> bit_count) as u16);
            bits &= (1 << bit_count) - 1;
        }
    }
    // What is left over only fills the last base64 character, with zeros.
    if bit_count >= 6 || bits != 0 {
        return None;
    }
    String::from_utf16(&code_units).ok()
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn ascii_names_are_unchanged() {
        assert_eq!(decode("Sent Messages"), "Sent Messages");
        assert_eq!(decode("Archive/2026-09"), "Archive/2026-09");
    }

    #[test]
    fn non_latin_names_are_decoded() {
        // The example of RFC 3501 §5.1.3.
        assert_eq!(
            decode("~peter/mail/&U,BTFw-/&ZeVnLIqe-"),
            "~peter/mail/台北/日本語"
        );
        // A character outside the basic plane, as a surrogate pair.
        assert_eq!(decode("&2D3eAA- Emoji"), "😀 Emoji");
    }

    #[test]
    fn an_escaped_ampersand_is_an_ampersand() {
        assert_eq!(decode("Tom &- Jerry"), "Tom & Jerry");
    }

    #[test]
    fn a_name_that_is_not_modified_utf7_is_returned_as_sent() {
        for name in [
            // The group never ends.
            "&U,BTFw",
            // The group ends inside a character.
            "&U,BTF-",
            // A character outside modified base64.
            "&U,B/TFw-",
            // Half of a surrogate pair.
            "&2D0-",
        ] {
            assert_eq!(decode(name), name);
        }
    }
}
