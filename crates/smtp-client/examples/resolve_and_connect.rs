//! End-to-end example: resolve MX for a domain, optionally open an SMTP
//! connection, say EHLO, then QUIT. No mail is actually sent.
//!
//! Run with:
//!   cargo run -p mailrs-smtp-client --example resolve_and_connect
//!   cargo run -p mailrs-smtp-client --example resolve_and_connect -- gmail.com
//!
//! By default only resolves MX and prints the list — no outbound TCP. Pass
//! `--connect` to also EHLO + QUIT against the primary MX. EHLO + QUIT is a
//! harmless courtesy probe that real MTAs perform routinely, but skip it on
//! shared hosting where outbound port 25 is firewalled.

use std::time::Duration;

use mailrs_smtp_client::{MxCache, SmtpConnection, TokioResolver, sort_mx_records};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let do_connect = args.iter().any(|a| a == "--connect");
    let domain = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "gmail.com".into());
    let helo = "client.example.org";

    println!("resolving MX for {domain}");

    let resolver = TokioResolver::builder_tokio()?.build()?;
    let cache = MxCache::new(Duration::from_secs(300));

    let mut records = cache.resolve(&resolver, &domain).await?;
    sort_mx_records(&mut records);

    if records.is_empty() {
        println!("  (no MX records found)");
        return Ok(());
    }
    for r in &records {
        let label = if r.exchange.is_empty() {
            "(null MX — RFC 7505, domain refuses mail)"
        } else {
            r.exchange.as_str()
        };
        println!("  {:>5}  {label}", r.priority);
    }

    if !do_connect {
        println!("\npass --connect to also EHLO/QUIT the primary MX");
        return Ok(());
    }

    let primary = records.first().expect("checked above");
    if primary.exchange.is_empty() {
        println!("\nnull MX — nothing to connect to");
        return Ok(());
    }

    println!("\nconnecting to {} on port 25 ...", primary.exchange);
    let mut conn = SmtpConnection::connect(&primary.exchange, 25).await?;
    println!("connected (tls = {})", conn.is_tls());

    println!("\n>>> EHLO {helo}");
    let resp = conn.ehlo(helo).await?;
    let first_line = resp.message();
    let first_line = first_line.lines().next().unwrap_or("");
    println!("<<< {} {first_line}", resp.code);
    if resp.has_extension("STARTTLS") {
        println!("    (STARTTLS advertised — could now upgrade with conn.starttls(...))");
    }

    println!("\n>>> QUIT");
    conn.quit().await?;
    println!("done.");
    Ok(())
}
