-- SPDX-FileCopyrightText: 2026 Andrey Mitin
-- SPDX-License-Identifier: GPL-3.0-or-later

-- The stored form of each account's folders and their messages
-- (specs/009-synchronization/data-model.md). The hash of this text is the store's
-- version: any change to it discards an existing store at start, since
-- nothing is converted before the first release.

-- A folder as the account's latest completed folder list left it, with what
-- it remembers between cycles. On Microsoft 365, `server_position` is where
-- the next round of changes starts and `fill_place` where an unfinished
-- first fill continues.
-- `synchronized` means that its latest cycle completed; without memberships
-- it is then empty.
CREATE TABLE folder (
    id INTEGER PRIMARY KEY,
    account TEXT NOT NULL,
    identity TEXT NOT NULL,
    name TEXT NOT NULL,
    parent TEXT,
    role TEXT CHECK (role IN (
        'inbox',
        'starred',
        'important',
        'junk',
        'trash',
        'archive',
        'drafts',
        'sent',
        'all_mail'
    )),
    selectable INTEGER NOT NULL CHECK (selectable IN (0, 1)),
    server_position TEXT,
    fill_place TEXT,
    synchronized INTEGER NOT NULL CHECK (synchronized IN (0, 1)),
    UNIQUE (account, identity)
) STRICT;

-- A message, once per account however many folders list it.
CREATE TABLE message (
    id INTEGER PRIMARY KEY,
    account TEXT NOT NULL,
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
        'text_not_returned',
        'not_downloaded'
    )),
    content_detail TEXT,
    UNIQUE (account, identity)
) STRICT;

-- A message's place in a folder; a folder's rows are ordered by the message's
-- received date.
CREATE TABLE membership (
    folder INTEGER NOT NULL REFERENCES folder (id) ON DELETE CASCADE,
    message INTEGER NOT NULL REFERENCES message (id) ON DELETE CASCADE,
    PRIMARY KEY (folder, message)
) STRICT;

CREATE INDEX membership_of_message ON membership (message);
