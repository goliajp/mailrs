//! Starting `mailrs-fastcore-ocr` for one attachment and reading its answer.
//!
//! The child sets its own memory, CPU and priority limits; this side adds
//! the one limit a process cannot set on itself, wall-clock time, and kills
//! a child that runs past it.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A child still running after this long is killed. A dense page took up
/// to 70 s on one arm64 core in measurement; twenty of them, on the
/// child's two threads, stay well inside this.
const WALL_LIMIT: Duration = Duration::from_secs(1800);
const POLL: Duration = Duration::from_millis(100);
const DEFAULT_MODELS: &str = "/usr/share/mailrs/ocr";

/// Where the child binary and the models are.
#[derive(Debug, Clone)]
pub(crate) struct OcrSetup {
    /// The `mailrs-fastcore-ocr` executable.
    pub bin: PathBuf,
    /// Directory holding `det.onnx` and `rec.onnx`.
    pub models: PathBuf,
    pub wall_limit: Duration,
}

/// `MAILRS_OCR_BIN` (default: `mailrs-fastcore-ocr` next to this binary)
/// and `MAILRS_OCR_MODEL_DIR` (default `/usr/share/mailrs/ocr`), or why
/// OCR is off.
pub(crate) fn setup() -> Result<OcrSetup, String> {
    let bin = match std::env::var_os("MAILRS_OCR_BIN") {
        Some(b) => PathBuf::from(b),
        None => std::env::current_exe()
            .map_err(|e| format!("locating this binary: {e}"))?
            .with_file_name("mailrs-fastcore-ocr"),
    };
    let models = std::env::var_os("MAILRS_OCR_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MODELS));
    if !bin.is_file() {
        return Err(format!("{} not found", bin.display()));
    }
    for name in ["det.onnx", "rec.onnx"] {
        if !models.join(name).is_file() {
            return Err(format!("{} not found", models.join(name).display()));
        }
    }
    Ok(OcrSetup {
        bin,
        models,
        wall_limit: WALL_LIMIT,
    })
}

/// What to read.
pub(crate) enum Job<'a> {
    Image,
    /// These pages, from 0.
    Pdf(&'a [usize]),
}

/// The child's answer.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Answer {
    pub text: String,
    pub too_small: bool,
}

/// Run the child on `bytes`. Blocking; call it off the async runtime.
pub(crate) fn run(setup: &OcrSetup, job: Job<'_>, bytes: &[u8]) -> Result<Answer, String> {
    let mut cmd = Command::new(&setup.bin);
    match job {
        Job::Image => cmd.arg("image"),
        Job::Pdf(pages) => {
            let list: Vec<String> = pages.iter().map(usize::to_string).collect();
            cmd.arg("pdf").arg(list.join(","))
        }
    };
    let mut child = cmd
        .env("MAILRS_OCR_MODEL_DIR", &setup.models)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("starting {}: {e}", setup.bin.display()))?;
    let (Some(mut stdin), Some(mut stdout), Some(mut stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err("child pipes missing".to_string());
    };
    let input = bytes.to_vec();
    // feed and drain on their own threads: a child blocked writing a full
    // pipe would otherwise never read the rest of its input
    let feeder = std::thread::spawn(move || stdin.write_all(&input));
    let out_reader = std::thread::spawn(move || {
        let mut v = Vec::new();
        stdout.read_to_end(&mut v).map(|_| v)
    });
    let err_reader = std::thread::spawn(move || {
        let mut v = Vec::new();
        stderr.read_to_end(&mut v).map(|_| v)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > setup.wall_limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("killed after {}s", setup.wall_limit.as_secs()));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(e) => return Err(format!("waiting for the child: {e}")),
        }
    };
    let _ = feeder.join();
    let out = out_reader
        .join()
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    let err = err_reader
        .join()
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    if !status.success() {
        let msg = String::from_utf8_lossy(&err);
        return Err(format!("{status}: {}", msg.trim()));
    }
    serde_json::from_slice(&out).map_err(|e| format!("reading the answer: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &std::path::Path, body: &str) -> PathBuf {
        let path = dir.join("child.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn setup(bin: PathBuf, wall_limit: Duration) -> OcrSetup {
        OcrSetup {
            bin,
            models: PathBuf::from("/nonexistent"),
            wall_limit,
        }
    }

    #[test]
    fn a_child_past_the_wall_clock_limit_is_killed() {
        let tmp = tempfile::tempdir().unwrap();
        let s = setup(
            script(tmp.path(), "cat >/dev/null; exec sleep 30"),
            Duration::from_secs(1),
        );
        let started = Instant::now();
        let err = run(&s, Job::Image, b"bytes").unwrap_err();
        assert!(err.starts_with("killed after"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_failing_child_reports_its_stderr() {
        let tmp = tempfile::tempdir().unwrap();
        let s = setup(
            script(tmp.path(), "cat >/dev/null; echo broken >&2; exit 3"),
            WALL_LIMIT,
        );
        let err = run(&s, Job::Image, b"bytes").unwrap_err();
        assert!(err.contains("broken"), "{err}");
    }

    /// Input larger than a pipe buffer is fed while the answer is read.
    #[test]
    fn a_large_input_and_its_answer_both_get_through() {
        let tmp = tempfile::tempdir().unwrap();
        let body = r#"n=$(wc -c); printf '{"text":"%s %s","too_small":false}' "$1" "$n""#;
        let s = setup(script(tmp.path(), body), WALL_LIMIT);
        let a = run(&s, Job::Pdf(&[0, 2]), &vec![b'x'; 1 << 20]).unwrap();
        assert_eq!(
            a.text.split_whitespace().collect::<Vec<_>>(),
            ["pdf", &(1u32 << 20).to_string()]
        );
    }
}
