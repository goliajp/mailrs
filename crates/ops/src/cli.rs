//! `mailrs-ops <command>` inside the daemon's container, over the Unix
//! socket in its data directory. Changes must say who makes them (`--by`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const USAGE: &str = "usage:
  mailrs-ops [serve]
  mailrs-ops status
  mailrs-ops history [--hours N]
  mailrs-ops silence add <rule> [--object O] --for <30m|2h|1d> --reason <text> --by <name>
  mailrs-ops silence remove <id> --by <name> [--reason <text>]
  mailrs-ops maintenance start [--for <dur>] --reason <text> --by <name>
  mailrs-ops maintenance extend --for <dur> --reason <text> --by <name>
  mailrs-ops maintenance end --reason <text> --by <name>
  mailrs-ops test-channels
  mailrs-ops report-key <process>     (reads MAILRS_OPS_REPORT_MASTER_KEY)
  mailrs-ops check-alive <state dir> <max age seconds>";

/// `30s`, `45m`, `2h`, `1d`, or plain seconds.
pub fn duration(s: &str) -> Option<i64> {
    let s = s.trim();
    let (num, unit) = match s.char_indices().last()? {
        (i, c) if c.is_ascii_alphabetic() => (&s[..i], c),
        _ => (s, 's'),
    };
    let n: i64 = num.parse().ok()?;
    let mult = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => return None,
    };
    Some(n * mult).filter(|n| *n > 0)
}

/// The value after `--name`.
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn socket() -> PathBuf {
    let dir = std::env::var("MAILRS_OPS_DATA_DIR").unwrap_or_else(|_| "/data".into());
    Path::new(&dir).join("ops.sock")
}

/// One request over the socket: `(status, body)`.
pub async fn call(
    path: &Path,
    method: &str,
    uri: &str,
    body: Option<&serde_json::Value>,
) -> Result<(u16, String), String> {
    let mut stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(|e| format!("{}: {e} (is the daemon running?)", path.display()))?;
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let request = format!(
        "{method} {uri} HTTP/1.1\r\nHost: ops\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let (head, rest) = text.split_once("\r\n\r\n").ok_or("no answer")?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("no status")?;
    Ok((status, rest.to_string()))
}

fn print_answer(answer: Result<(u16, String), String>) -> ExitCode {
    match answer {
        Ok((200..=299, body)) => {
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or(body)),
                Err(_) => println!("{body}"),
            }
            ExitCode::SUCCESS
        }
        Ok((status, body)) => {
            eprintln!("refused ({status}): {body}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn print_status(v: &serde_json::Value) {
    let when = |t: &serde_json::Value| {
        t.as_i64()
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
            .map_or("-".into(), |d| d.to_rfc3339())
    };
    println!(
        "last cycle: {} ({} ms)",
        when(&v["last_cycle"]["started_at"]),
        v["last_cycle"]["duration_ms"]
    );
    match v["unmonitored"].as_bool() {
        Some(true) => println!("external watch: NONE (MAILRS_OPS_HEARTBEAT_URL unset)"),
        _ => println!("external watch: configured"),
    }
    match &v["maintenance"] {
        serde_json::Value::Null => println!("maintenance: off"),
        m => println!(
            "maintenance: on until {} by {} ({})",
            when(&m["until"]),
            m["by"],
            m["reason"]
        ),
    }
    let alerts = v["alerts"].as_array().cloned().unwrap_or_default();
    let ok = alerts.iter().filter(|a| a["phase"] == "ok").count();
    for a in alerts.iter().filter(|a| a["phase"] != "ok") {
        println!(
            "  {:<10} {:<8} {:<32} {:<24} {}",
            a["phase"].as_str().unwrap_or(""),
            a["severity"].as_str().unwrap_or(""),
            a["rule"].as_str().unwrap_or(""),
            a["object"].as_str().unwrap_or(""),
            a["value"].as_str().or(a["why"].as_str()).unwrap_or(""),
        );
    }
    println!("{ok} alert(s) ok, {} not ok", alerts.len() - ok);
    for p in v["processes"].as_array().cloned().unwrap_or_default() {
        println!(
            "  {:<10} {:<18} starts {} ({}), last report {}s ago",
            p["process"].as_str().unwrap_or(""),
            p["instance"].as_str().unwrap_or(""),
            p["start_count"],
            p["start_count_source"].as_str().unwrap_or(""),
            p["age_secs"],
        );
    }
}

pub async fn run(args: &[String]) -> ExitCode {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let path = socket();
    let need = |name: &str| flag(args, name).map(str::to_string);
    match a.as_slice() {
        ["status"] => match call(&path, "GET", "/v1/state", None).await {
            Ok((200, body)) => match serde_json::from_str(&body) {
                Ok(v) => {
                    print_status(&v);
                    ExitCode::SUCCESS
                }
                Err(e) => print_answer(Err(e.to_string())),
            },
            other => print_answer(other),
        },
        ["history", ..] => {
            let hours = flag(args, "--hours")
                .and_then(|h| h.parse::<i64>().ok())
                .unwrap_or(24);
            let since = mailrs_ops_report::build::now() - hours * 3600;
            print_answer(call(&path, "GET", &format!("/v1/history?since={since}"), None).await)
        }
        ["silence", "add", rule, ..] => {
            let body = serde_json::json!({
                "rule": rule,
                "object": need("--object"),
                "for_secs": flag(args, "--for").and_then(duration),
                "reason": need("--reason").unwrap_or_default(),
                "by": need("--by"),
            });
            print_answer(call(&path, "POST", "/v1/silences", Some(&body)).await)
        }
        ["silence", "remove", id, ..] => {
            let enc = |s: &str| {
                s.replace('%', "%25")
                    .replace(' ', "%20")
                    .replace('&', "%26")
            };
            let uri = format!(
                "/v1/silences/{id}?by={}&reason={}",
                enc(&need("--by").unwrap_or_default()),
                enc(&need("--reason").unwrap_or_else(|| "removed".into()))
            );
            print_answer(call(&path, "DELETE", &uri, None).await)
        }
        ["maintenance", action @ ("start" | "extend" | "end"), ..] => {
            let body = serde_json::json!({
                "action": action,
                "for_secs": flag(args, "--for").and_then(duration),
                "reason": need("--reason").unwrap_or_default(),
                "by": need("--by"),
            });
            print_answer(call(&path, "POST", "/v1/maintenance", Some(&body)).await)
        }
        ["test-channels"] => print_answer(call(&path, "POST", "/v1/channels/test", None).await),
        ["report-key", process] => report_key(process),
        ["check-alive", dir, max] => check_alive(dir, max),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn report_key(process: &str) -> ExitCode {
    let Ok(master) = std::env::var("MAILRS_OPS_REPORT_MASTER_KEY") else {
        eprintln!("MAILRS_OPS_REPORT_MASTER_KEY is not set");
        return ExitCode::from(2);
    };
    match mailrs_ops_report::key::derive(master.as_bytes(), process) {
        Some(key) => {
            println!("{key}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!(
                "{process:?} is not a process name: {}",
                mailrs_ops_report::wire::PROCESSES.join(", ")
            );
            ExitCode::from(2)
        }
    }
}

/// The healthcheck of a process with no HTTP: its liveness file is recent.
fn check_alive(dir: &str, max: &str) -> ExitCode {
    let Ok(max) = max.parse::<i64>() else {
        eprintln!("max age {max:?} is not a number of seconds");
        return ExitCode::from(2);
    };
    let now = mailrs_ops_report::build::now();
    match mailrs_ops_report::counter::alive_age(Path::new(dir), now) {
        Some(age) if age <= max => ExitCode::SUCCESS,
        Some(age) => {
            eprintln!("last alive {age}s ago (limit {max}s)");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("no liveness file in {dir}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_people_write_them() {
        assert_eq!(duration("90"), Some(90));
        assert_eq!(duration("30m"), Some(1800));
        assert_eq!(duration("2h"), Some(7200));
        assert_eq!(duration("1d"), Some(86_400));
        assert_eq!(duration("0h"), None);
        assert_eq!(duration("2w"), None);
        assert_eq!(duration("h"), None);
    }
}
