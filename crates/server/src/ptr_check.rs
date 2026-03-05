use std::net::IpAddr;
use std::sync::Arc;

use hickory_resolver::TokioResolver;

/// score PTR names against the EHLO domain (pure function, no I/O)
/// 0.0 = at least one name matches, 1.0 = no match
pub fn ptr_score_from_names(names: &[String], ehlo_domain: &str) -> f64 {
    if names.is_empty() {
        return 1.0;
    }
    let ehlo_lower = ehlo_domain.to_lowercase();
    let matches = names.iter().any(|name| {
        let name_str = name.trim_end_matches('.').to_lowercase();
        name_str == ehlo_lower || name_str.ends_with(&format!(".{ehlo_lower}"))
    });
    if matches {
        0.0
    } else {
        1.0
    }
}

/// check client PTR record and return a spam score contribution
/// 0.0 = PTR matches EHLO, 1.0 = PTR doesn't match, 1.5 = no PTR
pub async fn check_client_ptr(resolver: &TokioResolver, ip: IpAddr, ehlo_domain: &str) -> f64 {
    // skip loopback/private
    if ip.is_loopback() {
        return 0.0;
    }

    match resolver.reverse_lookup(ip).await {
        Ok(names) => {
            let name_strs: Vec<String> = names.iter().map(|n| n.to_ascii()).collect();
            ptr_score_from_names(&name_strs, ehlo_domain)
        }
        Err(_) => 1.5,
    }
}

/// check PTR record for the server's public IP and warn if it doesn't match hostname
pub async fn check_ptr_record(resolver: &Arc<TokioResolver>, hostname: &str) {
    // try to discover public IP via DNS (use resolver to look up our own hostname)
    let addrs = match resolver.lookup_ip(hostname).await {
        Ok(addrs) => addrs,
        Err(e) => {
            eprintln!("warning: PTR check failed to resolve {hostname}: {e}");
            return;
        }
    };

    for addr in addrs.iter() {
        match resolver.reverse_lookup(addr).await {
            Ok(names) => {
                let matches = names.iter().any(|name| {
                    let name_str = name.to_ascii().trim_end_matches('.').to_lowercase();
                    name_str == hostname.to_lowercase()
                });
                if !matches {
                    let ptr_names: Vec<String> = names
                        .iter()
                        .map(|n| n.to_ascii().trim_end_matches('.').to_string())
                        .collect();
                    eprintln!(
                        "warning: PTR record for {addr} does not match hostname {hostname} (found: {})",
                        ptr_names.join(", ")
                    );
                } else {
                    eprintln!("PTR check OK: {addr} -> {hostname}");
                }
            }
            Err(e) => {
                eprintln!("warning: PTR lookup for {addr} failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_scores_zero() {
        let names = vec!["mail.example.com".to_string()];
        assert_eq!(ptr_score_from_names(&names, "mail.example.com"), 0.0);
    }

    #[test]
    fn subdomain_match_scores_zero() {
        let names = vec!["smtp.mail.example.com".to_string()];
        assert_eq!(ptr_score_from_names(&names, "mail.example.com"), 0.0);
    }

    #[test]
    fn no_match_scores_one() {
        let names = vec!["other.domain.com".to_string()];
        assert_eq!(ptr_score_from_names(&names, "mail.example.com"), 1.0);
    }

    #[test]
    fn empty_names_scores_one() {
        assert_eq!(ptr_score_from_names(&[], "mail.example.com"), 1.0);
    }

    #[test]
    fn case_insensitive_match() {
        let names = vec!["MAIL.EXAMPLE.COM".to_string()];
        assert_eq!(ptr_score_from_names(&names, "mail.example.com"), 0.0);
    }

    #[test]
    fn multiple_names_any_match() {
        let names = vec![
            "unrelated.host.net".to_string(),
            "mail.example.com".to_string(),
        ];
        assert_eq!(ptr_score_from_names(&names, "mail.example.com"), 0.0);
    }

    #[test]
    fn trailing_dot_stripped() {
        let names = vec!["mail.example.com.".to_string()];
        assert_eq!(ptr_score_from_names(&names, "mail.example.com"), 0.0);
    }

    #[test]
    fn ehlo_uppercase_ptr_lowercase() {
        let names = vec!["mail.example.com".to_string()];
        assert_eq!(ptr_score_from_names(&names, "MAIL.EXAMPLE.COM"), 0.0);
    }

    #[test]
    fn partial_domain_no_false_positive() {
        // "notexample.com" should NOT match ehlo "example.com"
        let names = vec!["notexample.com".to_string()];
        assert_eq!(ptr_score_from_names(&names, "example.com"), 1.0);
    }

    #[test]
    fn multiple_names_none_match() {
        let names = vec![
            "foo.bar.net".to_string(),
            "baz.qux.org".to_string(),
        ];
        assert_eq!(ptr_score_from_names(&names, "mail.example.com"), 1.0);
    }

    #[test]
    fn deep_subdomain_matches() {
        let names = vec!["a.b.c.example.com".to_string()];
        assert_eq!(ptr_score_from_names(&names, "example.com"), 0.0);
    }

    #[test]
    fn empty_ehlo_domain_no_match() {
        let names = vec!["mail.example.com".to_string()];
        assert_eq!(ptr_score_from_names(&names, ""), 1.0);
    }

    #[test]
    fn trailing_dot_on_both() {
        let names = vec!["mail.example.com.".to_string()];
        // ehlo with trailing dot — ptr name stripped, but ehlo stays as-is
        // "mail.example.com" != "mail.example.com." so no exact match
        // also ".mail.example.com." won't match as suffix
        assert_eq!(ptr_score_from_names(&names, "mail.example.com."), 1.0);
    }
}
