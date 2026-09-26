-- SPDX-FileCopyrightText: 2026 Andrey Mitin
-- SPDX-License-Identifier: GPL-3.0-or-later

-- The stored form of each account's Inbox (specs/007-mail-storage/data-model.md).
-- The hash of this text is the store's version: any change to it discards an
-- existing store at start, since nothing is converted before the first release.

-- A row means that a load of the account completed; without messages its
-- Inbox is empty.
CREATE TABLE inbox (
    account TEXT NOT NULL PRIMARY KEY
) STRICT;

-- The messages of a stored Inbox; `id` keeps the load's order, newest first.
CREATE TABLE message (
    id INTEGER PRIMARY KEY,
    account TEXT NOT NULL REFERENCES inbox (account) ON DELETE CASCADE,
    identity TEXT NOT NULL,
    subject TEXT,
    sender TEXT,
    recipients TEXT,
    received INTEGER,
    seen INTEGER NOT NULL CHECK (seen IN (0, 1)),
    content_kind TEXT NOT NULL CHECK (content_kind IN (
        'text',
        'plain_text_missing',
        'html_only',
        'encrypted',
        'smime',
        'unknown_charset',
        'unknown_encoding',
        'undecodable',
        'structure_unreadable',
        'text_not_returned'
    )),
    content_detail TEXT
) STRICT;

CREATE INDEX message_in_inbox ON message (account, id);
