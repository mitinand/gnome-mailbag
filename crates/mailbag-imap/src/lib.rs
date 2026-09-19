// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads the Inbox of an IMAP account over a verified GIO TLS connection.
//!
//! This crate owns the protocol: the secure connection, sign-in, read-only
//! commands and the message part structure with IMAP section numbers. It has no
//! notion of a mail provider and never decodes message content.
