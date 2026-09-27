//! Convert parsed mail facts to Lua values; no fraud decisions here.
use luna_jit::{Lua, LuaTable};
use mailrs_fraud::{
    Facts, Policy,
    impersonation::{address_of, display_name_of, fold},
};

pub(super) fn install(lua: &mut Lua, f: &Facts<'_>, p: &Policy) -> Result<(), String> {
    let table = lua.create_table();
    let result = fill(lua, table, f, p).and_then(|()| {
        lua.set_global("m", table)
            .map_err(|e| lua.vm().error_text(&e))
    });
    let _ = lua.unpin(table);
    result
}

fn fill(lua: &mut Lua, t: LuaTable, f: &Facts<'_>, p: &Policy) -> Result<(), String> {
    let domain = address_of(f.from)
        .and_then(|a| a.rsplit('@').next())
        .unwrap_or("")
        .trim()
        .trim_end_matches('>')
        .to_ascii_lowercase();
    let host = f.domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let reg = mailrs_fraud::brand::registrable(&host);
    let prefix = host.strip_suffix(&reg).unwrap_or("").trim_end_matches('.');
    let addr = match (f.from.rfind('<'), f.from.rfind('>')) {
        (Some(a), Some(b)) if a < b => &f.from[a + 1..b],
        _ => f.from.trim(),
    }
    .trim()
    .to_ascii_lowercase();
    let (local, minted_host) = addr.rsplit_once('@').unwrap_or(("", ""));
    let minted_reg = mailrs_fraud::brand::registrable(minted_host.trim_end_matches('.'));
    for (k, v) in [
        ("from", f.from.to_string()),
        ("from_domain", domain),
        ("display", fold(&display_name_of(f.from))),
        ("domain", f.domain.to_string()),
        ("registrable", f.registrable.to_string()),
        ("host_registrable", reg.clone()),
        ("host_prefix", prefix.to_string()),
        ("minted_local", local.trim().to_string()),
        (
            "minted_sld",
            minted_reg.split('.').next().unwrap_or("").to_string(),
        ),
        ("subject", f.subject.to_string()),
        ("subject_folded", fold(f.subject)),
        ("x_mailer", f.x_mailer.unwrap_or("").to_string()),
        ("to_display", f.to_display.to_string()),
        ("spf", f.spf.to_string()),
        ("dkim", f.dkim.to_string()),
        ("dmarc", f.dmarc.to_string()),
    ] {
        if v.len() > 64 * 1024 {
            return Err(format!("fact {k} exceeds 64 KiB"));
        }
        t.set(lua, k, v).map_err(|e| lua.vm().error_text(&e))?;
    }
    t.set(
        lua,
        "domain_seen",
        f.domain_seen.min(i64::MAX as u64) as i64,
    )
    .map_err(|e| lua.vm().error_text(&e))?;
    t.set(lua, "reply_rotation", i64::from(f.reply_rotation))
        .map_err(|e| lua.vm().error_text(&e))?;
    for (k, v) in [
        ("has_zero_width", f.has_zero_width),
        ("has_zero_width_in_name", f.has_zero_width_in_name),
        (
            "has_zero_width_inside_a_word",
            f.has_zero_width_inside_a_word,
        ),
        ("has_bidi_override", f.has_bidi_override),
        ("has_executable_attachment", f.has_executable_attachment),
        ("unauthenticated", f.unauthenticated),
        ("peer_is_private", f.peer_is_private),
        ("offers_the_reader_a_sum", f.offers_the_reader_a_sum),
        ("is_bulk", f.is_bulk),
    ] {
        t.set(lua, k, v).map_err(|e| lua.vm().error_text(&e))?;
    }
    for (k, list, domains) in [
        ("org_names", &p.org_names, false),
        ("account_names", &p.account_names, false),
        ("our_domains", &p.our_domains, true),
        ("allowed_domains", &p.allowed_domains, true),
    ] {
        let array = lua.create_table();
        let result = (|| {
            let mut i = 1i64;
            for s in list {
                let s = if domains {
                    s.trim().to_ascii_lowercase()
                } else {
                    s.clone()
                };
                if domains && s.is_empty() {
                    continue;
                }
                array.set(lua, i, s).map_err(|e| lua.vm().error_text(&e))?;
                i += 1;
            }
            t.set(lua, k, array).map_err(|e| lua.vm().error_text(&e))
        })();
        let _ = lua.unpin(array);
        result?;
    }
    Ok(())
}
