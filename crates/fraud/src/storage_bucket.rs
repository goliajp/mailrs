//! Links that go only to a cloud object-storage bucket with a generated
//! name.
//!
//! A phishing page can be uploaded to S3, Google Cloud Storage, Azure
//! Blob, R2 and the like in a minute, under the provider's own domain and
//! certificate, so the link carries none of the domain signals the other
//! rules read. What it does carry is the bucket's name: a kit makes one
//! up per campaign, and a made-up name looks made up. A company serving
//! its newsletter images from a bucket calls it after itself.
//!
//! Both halves are allow-lists. A host is a bucket only on a provider
//! listed here, in the virtual-hosted form where the bucket is the
//! leftmost label; and a name is generated only by one of four
//! structural tests. An unfamiliar provider, a path-style URL
//! (`s3.amazonaws.com/<bucket>/…`, where the host names no bucket) and a
//! readable name all read as *not* a random bucket.

/// Whether every host in `link_hosts` is a cloud storage bucket with a
/// generated name, and there is at least one. Hosts are lowercased, as
/// `mailrs_inbound::scan_input::link_hosts` gives them.
#[must_use]
pub fn only_random_buckets(link_hosts: &[String]) -> bool {
    !link_hosts.is_empty()
        && link_hosts
            .iter()
            .all(|h| bucket_of(h).is_some_and(name_is_generated))
}

/// The hosts, one per line, when [`only_random_buckets`] holds for them;
/// empty otherwise.
#[must_use]
pub fn bucket_hosts(link_hosts: &[String]) -> String {
    if only_random_buckets(link_hosts) {
        link_hosts.join("\n")
    } else {
        String::new()
    }
}

/// The bucket a host serves, when it is one of the listed providers'
/// virtual-hosted forms. Tencent's `<name>-<appid>` loses its appid,
/// which every Tencent bucket has and nobody chose.
fn bucket_of(host: &str) -> Option<&str> {
    let host = host.trim_end_matches('.');
    let (bucket, rest) = host.split_once('.')?;
    if bucket.is_empty() || !on_a_storage_provider(rest) {
        return None;
    }
    if rest.starts_with("cos.") && rest.ends_with(".myqcloud.com") {
        return match bucket.rsplit_once('-') {
            Some((name, appid))
                if !name.is_empty()
                    && !appid.is_empty()
                    && appid.bytes().all(|b| b.is_ascii_digit()) =>
            {
                Some(name)
            }
            _ => Some(bucket),
        };
    }
    Some(bucket)
}

/// Whether `rest`, the host after its leftmost label, is a provider's
/// bucket endpoint.
fn on_a_storage_provider(rest: &str) -> bool {
    let labels: Vec<&str> = rest.split('.').collect();
    let first = labels[0];
    let s3ish = first == "s3" || first.starts_with("s3-");
    let ends = |suffix: &str| rest.ends_with(&format!(".{suffix}"));
    // <bucket>.s3[.dualstack][.<region>].amazonaws.com[.cn], and the
    // s3-<region> / s3-website-<region> / s3-website.<region> forms
    (s3ish && (ends("amazonaws.com") || ends("amazonaws.com.cn")))
        || rest == "storage.googleapis.com"
        || rest == "blob.core.windows.net"
        || rest == "web.core.windows.net"
        // static websites: <account>.z13.web.core.windows.net
        || (labels.len() == 5
            && first.len() > 1
            && first.starts_with('z')
            && first[1..].bytes().all(|b| b.is_ascii_digit())
            && ends("web.core.windows.net"))
        || rest == "r2.dev"
        || ends("digitaloceanspaces.com")
        || (s3ish && ends("backblazeb2.com"))
        || (first.starts_with("oss-") && labels.len() == 3 && ends("aliyuncs.com"))
        || (first == "cos" && ends("myqcloud.com"))
        || (first == "obs" && ends("myhuaweicloud.com"))
        || (s3ish && (rest == "wasabisys.com" || ends("wasabisys.com")))
        || (labels.len() == 3 && ends("linodeobjects.com"))
        || (labels.len() == 3 && ends("your-objectstorage.com"))
}

/// Whether a bucket name was generated rather than chosen.
///
/// Only `[a-z0-9-]`, at least ten long, and one of:
///
/// - hexadecimal, with at least one digit, sixteen or more without the
///   hyphens (a hash or a UUID);
/// - only digits, ten or more;
/// - at least two letters and two digits, switching between letter and
///   digit at least three times (`a8k3f9`);
/// - a hyphen-free stretch of sixteen or more with a digit, whose
///   characters carry at least 3.8 bits each and whose letters are at
///   most a quarter vowels.
///
/// The last one is narrower than entropy alone, on purpose. Character
/// entropy rises with length, and readable names with a year in them
/// clear 3.8 bits: `cdn-images-tokyo-2026` is 3.98, `my-website-backup-01`
/// 3.88, `mycompanyassets2025backup` 3.86. Counting only within a
/// hyphen-free stretch drops the first two; the vowel share drops the
/// third, since words are about two-fifths vowels and a uniformly
/// random letter is one-fifth.
fn name_is_generated(name: &str) -> bool {
    if name.len() < 10
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return false;
    }
    let bare: String = name.chars().filter(|c| *c != '-').collect();
    let digits = bare.bytes().filter(u8::is_ascii_digit).count();
    let letters = bare.len() - digits;
    let hex = bare.len() >= 16 && digits > 0 && bare.bytes().all(|b| b.is_ascii_hexdigit());
    let numeric = letters == 0 && digits >= 10;
    let mixed = letters >= 2 && digits >= 2 && switches(name) >= 3;
    hex || numeric || mixed || name.split('-').any(dense)
}

/// Adjacent letter–digit pairs, either way round. A hyphen separates.
fn switches(name: &str) -> usize {
    name.as_bytes()
        .windows(2)
        .filter(|w| {
            (w[0].is_ascii_lowercase() && w[1].is_ascii_digit())
                || (w[0].is_ascii_digit() && w[1].is_ascii_lowercase())
        })
        .count()
}

/// A hyphen-free stretch that reads as noise: see [`name_is_generated`].
fn dense(segment: &str) -> bool {
    let letters: Vec<u8> = segment.bytes().filter(u8::is_ascii_lowercase).collect();
    let vowels = letters.iter().filter(|b| b"aeiou".contains(b)).count();
    segment.len() >= 16
        && segment.bytes().any(|b| b.is_ascii_digit())
        && entropy(segment) >= 3.8
        && vowels * 4 <= letters.len()
}

/// Shannon entropy of the characters of `s`, in bits per character.
fn entropy(s: &str) -> f64 {
    let mut counts = [0u32; 256];
    for b in s.bytes() {
        counts[usize::from(b)] += 1;
    }
    let n = s.len() as f64;
    counts
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = f64::from(*c) / n;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(h: &[&str]) -> Vec<String> {
        h.iter().map(|s| s.to_string()).collect()
    }

    /// The four structural forms of a generated name, each on a
    /// different provider.
    #[test]
    fn generated_names_on_every_form_of_provider_host() {
        for host in [
            "3f9a0c7e5b1d4a2f8e6c.s3.amazonaws.com",
            "x7k2m9qp4z8w.s3.us-east-1.amazonaws.com",
            "a8k3f9z2qq.s3-ap-northeast-1.amazonaws.com",
            "a8k3f9z2qq.s3-website-us-west-2.amazonaws.com",
            "a8k3f9z2qq.s3-website.eu-central-1.amazonaws.com",
            "a8k3f9z2qq.s3.dualstack.us-east-1.amazonaws.com",
            "a8k3f9z2qq.s3.cn-north-1.amazonaws.com.cn",
            "4829103746.storage.googleapis.com",
            "a8k3f9z2qq.blob.core.windows.net",
            "a8k3f9z2qq.z13.web.core.windows.net",
            "a8k3f9z2qq.web.core.windows.net",
            "pub-0a1b2c3d4e5f60718293a4b5c6d7e8f9.r2.dev",
            "a8k3f9z2qq.nyc3.digitaloceanspaces.com",
            "a8k3f9z2qq.s3.us-west-004.backblazeb2.com",
            "a8k3f9z2qq.oss-cn-hangzhou.aliyuncs.com",
            "a8k3f9z2qq-1250000000.cos.ap-guangzhou.myqcloud.com",
            "a8k3f9z2qq.obs.cn-north-4.myhuaweicloud.com",
            "a8k3f9z2qq.s3.wasabisys.com",
            "a8k3f9z2qq.s3.eu-central-1.wasabisys.com",
            "a8k3f9z2qq.us-east-1.linodeobjects.com",
            "a8k3f9z2qq.fsn1.your-objectstorage.com",
            // a dense stretch of low-vowel noise beside a readable word
            "promo-kqzvhwbtrmxpfj82.s3.amazonaws.com",
        ] {
            assert!(only_random_buckets(&hosts(&[host])), "{host}");
        }
    }

    /// Not in the corpus: companies name their buckets after themselves,
    /// with a year or a region in them often enough.
    #[test]
    fn a_readable_bucket_name_is_not_random() {
        for host in [
            "acme-newsletter-assets.s3.amazonaws.com",
            "acme-invoices-2024.s3.ap-northeast-1.amazonaws.com",
            "cdn-images-tokyo-2026.storage.googleapis.com",
            "my-website-backup-01.blob.core.windows.net",
            "mycompanyassets2025backup.s3.amazonaws.com",
            "marketing-campaign-2025-q3.nyc3.digitaloceanspaces.com",
            "newsletter2024images.s3.amazonaws.com",
            "examplebucket-1250000000.cos.ap-guangzhou.myqcloud.com",
            // hexadecimal letters only spell words
            "acceded-facade-decade.s3.amazonaws.com",
            // too short to say anything
            "a1b2c3.s3.amazonaws.com",
        ] {
            assert!(!only_random_buckets(&hosts(&[host])), "{host}");
        }
    }

    /// Not in the corpus: path-style URLs put the bucket in the path,
    /// and the host is the provider's own. Not handled, so not convicted.
    #[test]
    fn a_path_style_url_names_no_bucket() {
        for host in [
            "s3.amazonaws.com",
            "s3.us-east-1.amazonaws.com",
            "s3-ap-northeast-1.amazonaws.com",
            "storage.googleapis.com",
            "f004.backblazeb2.com",
        ] {
            assert!(!only_random_buckets(&hosts(&[host])), "{host}");
        }
    }

    /// Not in the corpus: one bucket among other links is a message
    /// that links somewhere else as well.
    #[test]
    fn a_bucket_beside_another_link_is_not_only_buckets() {
        assert!(!only_random_buckets(&hosts(&[
            "a8k3f9z2qq.s3.amazonaws.com",
            "www.example.com",
        ])));
        assert!(only_random_buckets(&hosts(&[
            "a8k3f9z2qq.s3.amazonaws.com",
            "4829103746.storage.googleapis.com",
        ])));
        assert!(!only_random_buckets(&[]));
    }

    /// A generated-looking label on a host that is not a listed storage
    /// endpoint is not a bucket.
    #[test]
    fn a_random_label_elsewhere_is_not_a_bucket() {
        for host in [
            "a8k3f9z2qq.example.com",
            "a8k3f9z2qq.amazonaws.com",
            "a8k3f9z2qq.ec2.amazonaws.com",
            "a8k3f9z2qq.googleapis.com",
            "a8k3f9z2qq.core.windows.net",
            "a8k3f9z2qq.r2.dev.example",
            "a8k3f9z2qq.oss-cn-hangzhou.aliyuncs.com.example",
        ] {
            assert!(!only_random_buckets(&hosts(&[host])), "{host}");
        }
    }
}
