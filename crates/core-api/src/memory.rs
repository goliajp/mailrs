//! This container's memory, read from cgroup v2.

use crate::types::MemoryWire;

/// `memory.current` and `memory.max` of the cgroup this process is in;
/// `None` when they cannot be read.
pub fn cgroup() -> Option<MemoryWire> {
    from_dir(std::path::Path::new("/sys/fs/cgroup"))
}

fn from_dir(dir: &std::path::Path) -> Option<MemoryWire> {
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
    let used_bytes = read("memory.current")?.trim().parse().ok()?;
    let limit_bytes = read("memory.max").and_then(|v| v.trim().parse().ok());
    Some(MemoryWire {
        used_bytes,
        limit_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_usage_and_limit_and_an_unlimited_cgroup() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("memory.current"), "2147483648\n").unwrap();
        std::fs::write(tmp.path().join("memory.max"), "4294967296\n").unwrap();
        assert_eq!(
            from_dir(tmp.path()),
            Some(MemoryWire {
                used_bytes: 2_147_483_648,
                limit_bytes: Some(4_294_967_296)
            })
        );
        std::fs::write(tmp.path().join("memory.max"), "max\n").unwrap();
        assert_eq!(from_dir(tmp.path()).unwrap().limit_bytes, None);
        std::fs::remove_file(tmp.path().join("memory.current")).unwrap();
        assert_eq!(from_dir(tmp.path()), None);
    }
}
