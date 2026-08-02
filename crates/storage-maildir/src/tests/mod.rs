//! Split by subject on 2026-08-02; every test moved verbatim.

mod cleanup;
mod flags;
mod ids;
mod layout;
mod pure;
mod scan;

fn tmpdir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}
