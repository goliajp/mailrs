//! Turn extracted facts into the Lua table `m`; no decisions here.

use luna_jit::vm::IntoValue;
use luna_jit::{Lua, LuaTable};
use mailrs_attachment_scan::PartFacts;
use mailrs_reputation::{Listing, Marks};

use crate::Input;

type R = Result<(), String>;

pub(super) fn install(lua: &mut Lua, input: &Input<'_>) -> R {
    let table = lua.create_table();
    let result = fill(lua, table, input).and_then(|()| {
        lua.set_global("m", table)
            .map_err(|e| lua.vm().error_text(&e))
    });
    let _ = lua.unpin(table);
    result
}

fn set<V: IntoValue>(lua: &mut Lua, t: LuaTable, k: &str, v: V) -> R {
    t.set(lua, k, v).map_err(|e| lua.vm().error_text(&e))
}

/// A child table, filled by `f` and stored in `parent` under `k`.
fn child(
    lua: &mut Lua,
    parent: LuaTable,
    k: Key<'_>,
    f: impl FnOnce(&mut Lua, LuaTable) -> R,
) -> R {
    let t = lua.create_table();
    let result = f(lua, t).and_then(|()| {
        match k {
            Key::Name(k) => parent.set(lua, k, t),
            Key::Index(i) => parent.set(lua, i, t),
        }
        .map_err(|e| lua.vm().error_text(&e))
    });
    let _ = lua.unpin(t);
    result
}

#[derive(Clone, Copy)]
enum Key<'a> {
    Name(&'a str),
    Index(i64),
}

fn fill(lua: &mut Lua, t: LuaTable, input: &Input<'_>) -> R {
    let rep = input.reputation;
    set(lua, t, "outside", input.outside)?;
    set(lua, t, "dmarc_fail", rep.dmarc_fail)?;
    set(lua, t, "spf_fail", rep.spf_fail)?;
    for (name, list) in [
        ("ip", &rep.ip),
        ("envelope", &rep.envelope),
        ("header", &rep.header),
        ("links", &rep.links),
    ] {
        child(lua, t, Key::Name(name), |lua, arr| listings(lua, arr, list))?;
    }
    if let Some(marks) = &rep.sender_marks {
        child(lua, t, Key::Name("sender_marks"), |lua, mt| {
            marks_table(lua, mt, marks)
        })?;
    }
    child(lua, t, Key::Name("link_marks"), |lua, arr| {
        for (i, marks) in rep.link_marks.iter().enumerate() {
            child(lua, arr, Key::Index(i as i64 + 1), |lua, mt| {
                marks_table(lua, mt, marks)
            })?;
        }
        Ok(())
    })?;
    child(lua, t, Key::Name("parts"), |lua, arr| {
        for (i, p) in input.parts.iter().enumerate() {
            child(lua, arr, Key::Index(i as i64 + 1), |lua, pt| {
                part(lua, pt, p)
            })?;
        }
        Ok(())
    })
}

fn listings(lua: &mut Lua, arr: LuaTable, list: &[Listing]) -> R {
    for (i, l) in list.iter().enumerate() {
        child(lua, arr, Key::Index(i as i64 + 1), |lua, lt| {
            set(lua, lt, "subject", l.subject.clone())?;
            set(lua, lt, "id", l.id.clone())?;
            set(lua, lt, "source", l.source.as_str().to_string())?;
            set(lua, lt, "age_days", l.age_days)?;
            set(lua, lt, "reason", l.reason.clone())
        })?;
    }
    Ok(())
}

fn marks_table(lua: &mut Lua, t: LuaTable, marks: &Marks) -> R {
    set(lua, t, "domain", marks.domain.clone())?;
    set(lua, t, "never", marks.never)?;
    child(lua, t, Key::Name("marks"), |lua, arr| {
        for (i, mk) in marks.marks.iter().enumerate() {
            child(lua, arr, Key::Index(i as i64 + 1), |lua, x| {
                set(lua, x, "id", mk.id.clone())?;
                set(lua, x, "source", mk.source.as_str().to_string())?;
                set(lua, x, "actor", mk.actor.clone())?;
                set(lua, x, "age_days", mk.age_days)
            })?;
        }
        Ok(())
    })
}

fn part(lua: &mut Lua, t: LuaTable, p: &PartFacts) -> R {
    set(lua, t, "part", p.part as i64)?;
    set(lua, t, "name", p.name.clone())?;
    set(lua, t, "extension", p.extension.clone())?;
    set(lua, t, "sha256", p.sha256.clone())?;
    set(lua, t, "truncated", p.truncated)?;
    set(lua, t, "html_base64_run", p.html_base64_run as i64)?;
    for (k, v) in [
        ("executable_name", &p.executable_name),
        ("double_extension", &p.double_extension),
        ("type_mismatch", &p.type_mismatch),
        ("archive_executable", &p.archive_executable),
        ("encrypted_archive", &p.encrypted_archive),
        ("unlistable_archive", &p.unlistable_archive),
        ("office_macros", &p.office_macros),
        ("office_object", &p.office_object),
        ("rtf_object", &p.rtf_object),
        ("pdf_script", &p.pdf_script),
        ("pdf_embedded", &p.pdf_embedded),
        ("html_writer", &p.html_writer),
        ("html_form", &p.html_form),
        ("svg_script", &p.svg_script),
    ] {
        if let Some(v) = v {
            set(lua, t, k, v.clone())?;
        }
    }
    if let Some(h) = &p.hash_listed {
        child(lua, t, Key::Name("hash_listed"), |lua, ht| {
            set(lua, ht, "id", h.id.clone())?;
            set(lua, ht, "age_days", h.age_days)?;
            set(lua, ht, "reason", h.reason.clone())
        })?;
    }
    Ok(())
}
