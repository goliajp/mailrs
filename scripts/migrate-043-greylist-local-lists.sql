-- migrate-043: greylist local white/black lists (Phase 2).
--
-- Phase 1 (v1.7.140) added a remote sender-domain whitelist synced from a
-- github raw URL. Phase 2 layers operator-controlled per-server white and
-- black lists on top, with the policy "any local black-kind hit beats any
-- local white-kind hit, both of which beat the remote whitelist".
--
-- The UNIQUE (kind, value) constraint (no `list` in the tuple) is the
-- schema-level mutex: a given key can only live on one list at a time. To
-- move an entry, DELETE + INSERT — there is no PATCH/PUT in the admin API
-- by design (smaller surface, simpler audit).

CREATE TABLE IF NOT EXISTS greylist_local_lists (
    id          BIGSERIAL PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('domain', 'email', 'cidr')),
    list        TEXT NOT NULL CHECK (list IN ('white', 'black')),
    value       TEXT NOT NULL,
    note        TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by  TEXT,
    UNIQUE (kind, value)
);

CREATE INDEX IF NOT EXISTS greylist_local_lists_kind_idx ON greylist_local_lists (kind);
