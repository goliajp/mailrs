//! The case for the storage-bucket rule.

use mailrs_fraud::Facts;

pub(super) fn cases(base: Facts<'static>) -> Vec<super::Case> {
    vec![(
        "unvouched-sender-links-only-to-a-bucket",
        Facts {
            spf: "none",
            dkim: "fail",
            dmarc: "none",
            unauthenticated: true,
            has_links: true,
            links_only_to_random_buckets: true,
            bucket_hosts: "a8k3f9z2qq.s3.amazonaws.com",
            ..base
        },
        &[
            ("domain", "sender.example"),
            ("spf", "none"),
            ("dkim", "fail"),
            ("dmarc", "none"),
        ],
    )]
}
