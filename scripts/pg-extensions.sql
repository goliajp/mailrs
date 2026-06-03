-- PG-only extension setup. Mounted as docker-entrypoint-initdb.d/00-pg-extensions.sql
-- so it runs BEFORE init-schema.sql on a fresh postgres-image database.
--
-- Phase D-pre #4 separated this from init-schema.sql so the same schema file
-- can run on both PG and SPG. SPG ships with pgvector-style VECTOR support
-- builtin (no extension system); only PG needs this CREATE EXTENSION.
CREATE EXTENSION IF NOT EXISTS vector;
