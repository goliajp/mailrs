//! Commands run on the host with `docker exec`, for what no HTTP route
//! may do.
//!
//! `agent-key create` makes a send-only key for any account. A route that
//! mints keys for other accounts is what a leaked admin key would reach
//! for, so this exists only here, behind access to the host.

use std::process::ExitCode;

const USAGE: &str =
    "usage: mailrs-webapi agent-key create --owner <address> --scope mail:send --name <name>";

/// The only scope this command grants.
const SCOPE: &str = "mail:send";

/// `Some` when `args` (without the program name) is a command, with how it
/// ended; `None` when the server should start.
pub async fn run(args: &[String]) -> Option<ExitCode> {
    match args.first().map(String::as_str) {
        Some("agent-key") => Some(agent_key(&args[1..]).await),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Create {
    owner: String,
    name: String,
}

fn parse_create(args: &[String]) -> Result<Create, String> {
    let Some(("create", flags)) = args.split_first().map(|(c, f)| (c.as_str(), f)) else {
        return Err(USAGE.into());
    };
    let (mut owner, mut scope, mut name) = (None, None, None);
    let mut it = flags.iter();
    while let Some(flag) = it.next() {
        let slot = match flag.as_str() {
            "--owner" => &mut owner,
            "--scope" => &mut scope,
            "--name" => &mut name,
            _ => return Err(format!("unknown argument {flag}\n{USAGE}")),
        };
        let Some(value) = it.next() else {
            return Err(format!("{flag} needs a value\n{USAGE}"));
        };
        if slot.replace(value.trim().to_string()).is_some() {
            return Err(format!("{flag} given twice"));
        }
    }
    match scope.as_deref() {
        Some(SCOPE) => {}
        Some(other) => return Err(format!("scope {other} is not granted here; only {SCOPE}")),
        None => return Err(format!("--scope {SCOPE} is required\n{USAGE}")),
    }
    let owner = owner.filter(|o| !o.is_empty()).ok_or(USAGE)?;
    let name = name.filter(|n| !n.is_empty()).ok_or(USAGE)?;
    Ok(Create {
        owner: owner.to_ascii_lowercase(),
        name,
    })
}

async fn agent_key(args: &[String]) -> ExitCode {
    let create = match parse_create(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    match create_send_key(&create.owner, &create.name).await {
        Ok(secret) => {
            println!("{secret}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Make a `mail:send` key for `owner`, an account, and record it in the
/// audit log; the secret. Reads the core and kevy addresses from the
/// environment, as the server does.
pub async fn create_send_key(owner: &str, name: &str) -> Result<String, String> {
    let create = Create {
        owner: owner.to_ascii_lowercase(),
        name: name.to_string(),
    };
    let create = &create;
    let state = crate::WebState::from_env();
    let accounts = state
        .core
        .list_accounts()
        .await
        .map_err(|e| format!("could not list accounts: {e}"))?;
    if !accounts
        .items
        .iter()
        .any(|a| a.address.eq_ignore_ascii_case(&create.owner))
    {
        return Err(format!("{} is not an account", create.owner));
    }
    let (owner, name) = (create.owner.clone(), create.name.clone());
    let (id, secret) = crate::handlers::kevy_util::with_kevy(move |c| {
        crate::handlers::apps_keys::mint_agent_key(c, &owner, &name, &[SCOPE.to_string()])
    })
    .map_err(|_| "could not write the key".to_string())?;
    let detail = format!(
        "id={id} prefix={} name={} scope={SCOPE}",
        &secret[..8],
        create.name
    );
    if crate::handlers::audit::record_required("host", "agent_key.create", &create.owner, &detail)
        .is_err()
    {
        // unrecorded, so it must not work: without its record the secret
        // index resolves to nothing
        let (owner, field) = (create.owner.clone(), id.to_string());
        let revoked = crate::handlers::kevy_util::with_kevy(move |c| {
            Ok(c.hdel(
                format!("agent:keys:{owner}").as_bytes(),
                &[field.as_bytes()],
            )?)
        });
        return Err(format!(
            "the audit line could not be written; key {id} {}",
            match revoked {
                Ok(_) => "was revoked",
                Err(_) => "could NOT be revoked, remove it from the key list",
            }
        ));
    }
    eprintln!("created key {id} ({}) for {}", &secret[..8], create.owner);
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn create_takes_an_owner_a_name_and_only_the_send_scope() {
        assert_eq!(
            parse_create(&args(&[
                "create",
                "--owner",
                "LiHao@golia.jp",
                "--scope",
                "mail:send",
                "--name",
                "weekly report",
            ])),
            Ok(Create {
                owner: "lihao@golia.jp".into(),
                name: "weekly report".into(),
            })
        );
        for refused in [
            args(&[
                "create",
                "--owner",
                "a@x",
                "--scope",
                "mail:write",
                "--name",
                "n",
            ]),
            args(&[
                "create", "--owner", "a@x", "--scope", "admin", "--name", "n",
            ]),
            args(&["create", "--owner", "a@x", "--name", "n"]),
            args(&["create", "--owner", "a@x", "--scope", "mail:send"]),
            args(&["create", "--scope", "mail:send", "--name", "n"]),
            args(&["create", "--owner", "a@x", "--scope", "mail:send", "--name"]),
            args(&[
                "create",
                "--owner",
                "a@x",
                "--owner",
                "b@x",
                "--scope",
                "mail:send",
                "--name",
                "n",
            ]),
            args(&[
                "create",
                "--owner",
                "a@x",
                "--scope",
                "mail:send",
                "--name",
                "n",
                "--x",
            ]),
            args(&["delete", "--owner", "a@x"]),
            args(&[]),
        ] {
            assert!(parse_create(&refused).is_err(), "{refused:?}");
        }
    }
}
