-- CalDAV / CardDAV tables

-- calendars
CREATE TABLE IF NOT EXISTS calendars (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT 'Default',
    color TEXT NOT NULL DEFAULT '#4285f4',
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(account_address, name)
);

-- calendar events (iCalendar objects)
CREATE TABLE IF NOT EXISTS calendar_events (
    id BIGSERIAL PRIMARY KEY,
    calendar_id BIGINT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    uid TEXT NOT NULL,
    etag TEXT NOT NULL,
    icalendar TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    dtstart TIMESTAMPTZ,
    dtend TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(calendar_id, uid)
);

-- address books
CREATE TABLE IF NOT EXISTS address_books (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT 'Default',
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(account_address, name)
);

-- contacts (vCard objects)
CREATE TABLE IF NOT EXISTS contacts (
    id BIGSERIAL PRIMARY KEY,
    address_book_id BIGINT NOT NULL REFERENCES address_books(id) ON DELETE CASCADE,
    uid TEXT NOT NULL,
    etag TEXT NOT NULL,
    vcard TEXT NOT NULL,
    fn_name TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(address_book_id, uid)
);

CREATE INDEX IF NOT EXISTS idx_calendar_events_calendar ON calendar_events(calendar_id);
CREATE INDEX IF NOT EXISTS idx_calendar_events_uid ON calendar_events(uid);
CREATE INDEX IF NOT EXISTS idx_contacts_book ON contacts(address_book_id);
CREATE INDEX IF NOT EXISTS idx_contacts_uid ON contacts(uid);
