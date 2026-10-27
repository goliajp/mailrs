//! Commands run on the host with `docker exec`, for what no HTTP route
//! may do.
//!
//! `agent-key create` makes a send-only key for any account, and
//! `agent-key revoke` removes any account's key. A route that mints keys
//! for other accounts is what a leaked admin key would reach for, so this
//! exists only here, behind access to the host.

use std::process::ExitCode;

const USAGE: &str =
    "usage: mailrs-webapi agent-key create --owner <address> --scope mail:send --name <name>
       mailrs-webapi agent-key revoke --owner <address> --id <id>";

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
enum Command {
    Create { owner: String, name: String },
    Revoke { owner: String, id: i64 },
}

fn parse(args: &[String]) -> Result<Command, String> {
    let Some((verb, flags)) = args.split_first() else {
        return Err(USAGE.into());
    };
    let names: &[&str] = match verb.as_str() {
        "create" => &["--owner", "--scope", "--name"],
        "revoke" => &["--owner", "--id"],
        _ => return Err(USAGE.into()),
    };
    let mut given: Vec<(&str, String)> = Vec::new();
    let mut it = flags.iter();
    while let Some(flag) = it.next() {
        let Some(name) = names.iter().find(|n| **n == flag.as_str()) else {
            return Err(format!("unknown argument {flag}\n{USAGE}"));
        };
        let Some(value) = it.next() else {
            return Err(format!("{flag} needs a value\n{USAGE}"));
        };
        if given.iter().any(|(n, _)| n == name) {
            return Err(format!("{flag} given twice"));
        }
        given.push((name, value.trim().to_string()));
    }
    let get = |name: &str| {
        given
            .iter()
            .find(|(n, v)| *n == name && !v.is_empty())
            .map(|(_, v)| v.clone())
    };
    let owner = get("--owner").ok_or(USAGE)?.to_ascii_lowercase();
    match verb.as_str() {
        "create" => {
            match get("--scope").as_deref() {
                Some(SCOPE) => {}
                Some(other) => {
                    return Err(format!("scope {other} is not granted here; only {SCOPE}"));
                }
                None => return Err(format!("--scope {SCOPE} is required\n{USAGE}")),
            }
            let name = get("--name").ok_or(USAGE)?;
            Ok(Command::Create { owner, name })
        }
        _ => {
            let id = get("--id")
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or(format!("--id must be a key id\n{USAGE}"))?;
            Ok(Command::Revoke { owner, id })
        }
    }
}

async fn agent_key(args: &[String]) -> ExitCode {
    let command = match parse(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let outcome = match command {
        Command::Create { owner, name } => create_send_key(&owner, &name)
            .await
            .map(|secret| println!("{secret}")),
        Command::Revoke { owner, id } => revoke_key(&owner, id).await,
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Revoke `owner`'s key `id` and record it in the audit log. An unknown
/// key is an error, so a typo does not read as done.
pub async fn revoke_key(owner: &str, id: i64) -> Result<(), String> {
    let owner = owner.to_ascii_lowercase();
    let o = owner.clone();
    let record = crate::handlers::kevy_util::with_kevy(move |c| {
        crate::handlers::apps_keys::revoke_agent_key(c, &o, id)
    })
    .map_err(|_| "could not revoke the key".to_string())?
    .ok_or(format!("{owner} has no key {id}"))?;
    let record: serde_json::Value = serde_json::from_slice(&record).unwrap_or_default();
    let detail = format!(
        "id={id} prefix={} name={}",
        record["prefix"].as_str().unwrap_or(""),
        record["name"].as_str().unwrap_or("")
    );
    crate::handlers::audit::record_required("host", "agent_key.revoke", &owner, &detail)
        .map_err(|_| format!("key {id} was revoked, but the audit line could not be written"))?;
    eprintln!("revoked key {id} of {owner}");
    Ok(())
}

/// Make a `mail:send` key for `owner`, an account, and record it in the
/// audit log; the secret. Reads the core and kevy addresses from the
/// environment, as the server does.
pub async fn create_send_key(owner: &str, name: &str) -> Result<String, String> {
    let owner = owner.to_ascii_lowercase();
    let state = crate::WebState::from_env();
    let accounts = state
        .core
        .list_accounts()
        .await
        .map_err(|e| format!("could not list accounts: {e}"))?;
    if !accounts
        .items
        .iter()
        .any(|a| a.address.eq_ignore_ascii_case(&owner))
    {
        return Err(format!("{owner} is not an account"));
    }
    let (o, n) = (owner.clone(), name.to_string());
    let (id, secret) = crate::handlers::kevy_util::with_kevy(move |c| {
        crate::handlers::apps_keys::mint_agent_key(c, &o, &n, &[SCOPE.to_string()])
    })
    .map_err(|_| "could not write the key".to_string())?;
    let detail = format!("id={id} prefix={} name={name} scope={SCOPE}", &secret[..8]);
    if crate::handlers::audit::record_required("host", "agent_key.create", &owner, &detail).is_err()
    {
        // unrecorded, so it must not work
        let o = owner.clone();
        let revoked = crate::handlers::kevy_util::with_kevy(move |c| {
            crate::handlers::apps_keys::revoke_agent_key(c, &o, id)
        });
        return Err(format!(
            "the audit line could not be written; key {id} {}",
            match revoked {
                Ok(_) => "was revoked",
                Err(_) => "could NOT be revoked, remove it from the key list",
            }
        ));
    }
    eprintln!("created key {id} ({}) for {owner}", &secret[..8]);
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
            parse(&args(&[
                "create",
                "--owner",
                "LiHao@golia.jp",
                "--scope",
                "mail:send",
                "--name",
                "weekly report",
            ])),
            Ok(Command::Create {
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
            args(&[
                "create",
                "--owner",
                "a@x",
                "--scope",
                "mail:send",
                "--id",
                "1",
            ]),
            args(&["delete", "--owner", "a@x"]),
            args(&[]),
        ] {
            assert!(parse(&refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn revoke_takes_an_owner_and_a_numeric_id() {
        assert_eq!(
            parse(&args(&[
                "revoke",
                "--owner",
                "Insight-Report@golia.jp",
                "--id",
                "1"
            ])),
            Ok(Command::Revoke {
                owner: "insight-report@golia.jp".into(),
                id: 1,
            })
        );
        for refused in [
            args(&["revoke", "--owner", "a@x"]),
            args(&["revoke", "--id", "1"]),
            args(&["revoke", "--owner", "a@x", "--id", "mk_cf27c"]),
            args(&["revoke", "--owner", "a@x", "--id", "1", "--name", "n"]),
            args(&["revoke", "--owner", "a@x", "--id", "1", "--id", "2"]),
        ] {
            assert!(parse(&refused).is_err(), "{refused:?}");
        }
    }
}
