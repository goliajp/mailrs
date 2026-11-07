//! The cases for the Japanese account and account-change rules.

use mailrs_fraud::Facts;
use mailrs_fraud::payment::find;
use mailrs_fraud::relation::Relation;

pub(super) fn cases(base: Facts<'static>) -> Vec<super::Case> {
    let jp = find("お振込先：ミツビシ銀行 渋谷支店 普通 1234567");
    vec![
        (
            "bec-jp-account-from-a-stranger",
            Facts {
                payment: jp.clone(),
                relation: Relation {
                    first_contact: true,
                    ..Relation::default()
                },
                ..base.clone()
            },
            &[
                ("account", "ミツビシ銀行 渋谷支店 普通 ****567"),
                ("domain", "sender.example"),
            ],
        ),
        (
            "bec-account-changed",
            Facts {
                payment: jp,
                relation: Relation {
                    account_changed: true,
                    ..Relation::default()
                },
                ..base
            },
            &[
                ("account", "ミツビシ銀行 渋谷支店 普通 ****567"),
                ("domain", "sender.example"),
            ],
        ),
    ]
}
