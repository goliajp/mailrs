//! The settings the System Config page shows, and which of them a write
//! can change.
//!
//! Only a key some process reads while it runs is editable here. A key
//! is listed as `editable` only together with that reader;
//! `scripts/check-inert-fields.sh` fails when an editable key is named by
//! no file outside webapi that also names `admin:system-config`.

use mailrs_smtp_proto::MAX_MESSAGE_SIZE;

pub(super) struct Setting {
    pub(super) key: &'static str,
    pub(super) group: &'static str,
    pub(super) description: &'static str,
    pub(super) kind: Kind,
}

pub(super) enum Kind {
    /// Read once at process start from `env_var` on the compose service
    /// `service`. `default` is that process's built-in value; `None` when
    /// webapi's own environment cannot show the value.
    Boot {
        env_var: &'static str,
        service: &'static str,
        default: Option<&'static str>,
    },
    /// Read while running; stored in `admin:system-config`.
    Editable,
}

pub(super) const MAX_MESSAGE_SIZE_BYTES: &str = "max_message_size_bytes";

pub(super) const CATALOG: &[Setting] = &[
    Setting {
        key: "hostname",
        group: "smtp",
        description: "Public SMTP hostname (greeting, HELO, Received headers)",
        kind: Kind::Boot {
            env_var: "MAILRS_HOSTNAME",
            service: "receiver, fastcore, webapi-fc",
            default: Some("mx.mailrs.local"),
        },
    },
    Setting {
        key: "smtp_port",
        group: "smtp",
        description: "Inbound SMTP port on the receiver",
        kind: Kind::Boot {
            env_var: "MAILRS_PORT",
            service: "receiver",
            default: Some("2525"),
        },
    },
    Setting {
        key: "submission_port",
        group: "smtp",
        description: "Authenticated submission port on the receiver",
        kind: Kind::Boot {
            env_var: "MAILRS_SUBMISSION_PORT",
            service: "receiver",
            default: Some("2587"),
        },
    },
    Setting {
        key: "imap_port",
        group: "imap",
        description: "IMAP listen address on fastcore",
        kind: Kind::Boot {
            env_var: "MAILRS_IMAP_BIND",
            service: "fastcore",
            default: Some("0.0.0.0:143"),
        },
    },
    Setting {
        key: "tls_enabled",
        group: "security",
        description: "TLS on 465 / 587 / 993 / 995 and STARTTLS on 25: on when both \
                      certificate paths are set",
        kind: Kind::Boot {
            env_var: "MAILRS_TLS_CERT, MAILRS_TLS_KEY",
            service: "receiver, fastcore",
            default: None,
        },
    },
    Setting {
        key: MAX_MESSAGE_SIZE_BYTES,
        group: "smtp",
        description: "Largest message the receiver accepts, in bytes. New connections \
                      use a new value within a minute",
        kind: Kind::Editable,
    },
];

pub(super) fn find(key: &str) -> Option<&'static Setting> {
    CATALOG.iter().find(|s| s.key == key)
}

pub(super) fn editable_default(key: &str) -> String {
    match key {
        MAX_MESSAGE_SIZE_BYTES => MAX_MESSAGE_SIZE.to_string(),
        _ => String::new(),
    }
}

pub(super) fn restart_note(env_var: &str, service: &str) -> String {
    format!(
        "Set by {env_var} on the {service} service in docker-compose and read when the \
         process starts. Changing it means editing compose and restarting the container."
    )
}

/// Why `value` cannot be stored under `key`, if it cannot.
pub(super) fn refusal(key: &str, value: &str) -> Option<String> {
    if key == "local_domains" {
        return Some(
            "Local domains are managed on the Domains page: a domain added there is \
             received within a minute"
                .into(),
        );
    }
    let Some(setting) = find(key) else {
        return Some(format!(
            "No process reads {key}. Only {MAX_MESSAGE_SIZE_BYTES} can be changed here."
        ));
    };
    match setting.kind {
        Kind::Boot {
            env_var, service, ..
        } => Some(restart_note(env_var, service)),
        Kind::Editable => match value.trim().parse::<u64>() {
            Ok(n) if (1..=MAX_MESSAGE_SIZE).contains(&n) => None,
            _ => Some(format!(
                "{key} must be a whole number of bytes from 1 to {MAX_MESSAGE_SIZE}, the \
                 receiver's built-in limit"
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_settings_local_domains_and_unknown_keys_are_refused() {
        for key in [
            "hostname",
            "smtp_port",
            "submission_port",
            "imap_port",
            "tls_enabled",
        ] {
            let why = refusal(key, "x").expect(key);
            assert!(why.contains("restarting"), "{key}: {why}");
        }
        assert!(
            refusal("local_domains", "a.example")
                .unwrap()
                .contains("Domains page")
        );
        assert!(
            refusal("made_up", "1")
                .unwrap()
                .contains("No process reads")
        );
    }

    #[test]
    fn the_size_limit_takes_a_byte_count_up_to_the_built_in_limit() {
        assert_eq!(refusal(MAX_MESSAGE_SIZE_BYTES, "1048576"), None);
        assert_eq!(
            refusal(MAX_MESSAGE_SIZE_BYTES, &MAX_MESSAGE_SIZE.to_string()),
            None
        );
        for bad in ["0", "-1", "10MB", "", &(MAX_MESSAGE_SIZE + 1).to_string()] {
            assert!(refusal(MAX_MESSAGE_SIZE_BYTES, bad).is_some(), "{bad:?}");
        }
    }
}
