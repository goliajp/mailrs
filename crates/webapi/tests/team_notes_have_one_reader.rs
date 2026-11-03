//! Internal notes reach people through one module each side: the notes
//! routes here and the notes panel in the web client. Nothing that builds,
//! quotes, exports or sends mail, and nothing that prepares a model's
//! input, reads the store they live in. A new reader shows up here first.

use std::path::{Path, PathBuf};

fn sources(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).expect("dir").flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if !matches!(name, "__tests__" | "tests" | "target" | "node_modules") {
                sources(&p, exts, out);
            }
        } else if exts.iter().any(|x| name.ends_with(x)) && !name.contains("test") {
            out.push(p);
        }
    }
}

fn readers(root: &Path, exts: &[&str], needles: &[&str]) -> Vec<String> {
    let mut files = Vec::new();
    sources(root, exts, &mut files);
    let mut out: Vec<String> = files
        .into_iter()
        .filter(|f| {
            let src = std::fs::read_to_string(f).unwrap_or_default();
            needles.iter().any(|n| src.contains(n))
        })
        .map(|f| f.strip_prefix(root).unwrap_or(&f).display().to_string())
        .collect();
    out.sort();
    out
}

#[test]
fn only_the_notes_routes_read_the_notes() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let found = readers(
        &crates,
        &[".rs"],
        &[
            "shared_inbox::notes",
            "shared_inbox::{notes",
            "notes::list(",
            "deleted_original(",
        ],
    );
    assert_eq!(
        found,
        vec![
            "core-sidestate/src/families/shared_inbox/notes.rs",
            "webapi/src/handlers/shared_inbox/notes.rs",
        ],
    );
}

#[test]
fn only_the_notes_panel_shows_them_in_the_web_client() {
    let web = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src");
    let found = readers(
        &web,
        &[".ts", ".tsx"],
        &["wireListNotes", "wireReadDeletedNote", "/notes`"],
    );
    assert_eq!(
        found,
        vec![
            "components/admin-deleted-note.tsx",
            "hooks/use-team.ts",
            "wire/endpoints/shared.ts",
        ],
    );
}
