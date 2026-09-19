// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Chooses the readable text parts of a received message and decodes them.
//!
//! This crate works on MIME part descriptions and raw MIME entities through
//! mail-parser. It knows nothing about IMAP, GIO or the user interface.
