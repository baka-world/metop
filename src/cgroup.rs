//! cgroup v2 memory accounting, read from /sys/fs/cgroup.
//!
//! Values are hierarchical: a parent's `memory.current` already includes its
//! descendants, so the flat list is sorted by current instead of summed.

use std::fs;
use std::path::{Path, PathBuf};

pub const ROOT: &str = "/sys/fs/cgroup";
const MAX_DEPTH: usize = 10;

#[derive(Clone, Debug, Default)]
pub struct CgroupMem {
    /// "/" for the root, otherwise "/user.slice/user-1000.slice/session-2.scope".
    pub path: String,
    pub current_kb: u64,
    pub anon_kb: u64,
    pub file_kb: u64,
    pub kernel_kb: u64,
    pub slab_kb: u64,
    pub shmem_kb: u64,
    /// `memory.max`; `None` when unlimited ("max") or unreadable.
    pub limit_kb: Option<u64>,
    pub oom_kills: u64,
}

/// Pure parser so the format can be tested without a cgroup filesystem.
pub fn parse_stat(text: &str, current_bytes: u64) -> CgroupMem {
    let mut out = CgroupMem {
        current_kb: current_bytes / 1024,
        ..Default::default()
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let Ok(bytes) = value.trim().parse::<u64>() else {
            continue;
        };
        match key {
            "anon" => out.anon_kb = bytes / 1024,
            "file" => out.file_kb = bytes / 1024,
            "kernel" => out.kernel_kb = bytes / 1024,
            "slab" => out.slab_kb = bytes / 1024,
            "shmem" => out.shmem_kb = bytes / 1024,
            _ => {}
        }
    }
    out
}

/// `memory.max` is either a byte count or the literal "max".
pub fn parse_limit(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok().map(|b| b / 1024)
}

pub fn parse_oom_kills(text: &str) -> u64 {
    text.lines()
        .find_map(|l| l.strip_prefix("oom_kill "))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

fn read_one(dir: &Path) -> Option<CgroupMem> {
    let current: u64 = fs::read_to_string(dir.join("memory.current"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let stat = fs::read_to_string(dir.join("memory.stat")).unwrap_or_default();
    let mut cg = parse_stat(&stat, current);
    cg.limit_kb = fs::read_to_string(dir.join("memory.max"))
        .ok()
        .and_then(|t| parse_limit(&t));
    cg.oom_kills = fs::read_to_string(dir.join("memory.events"))
        .map(|t| parse_oom_kills(&t))
        .unwrap_or(0);
    Some(cg)
}

fn rel_path(root: &Path, dir: &Path) -> String {
    let rel = dir.strip_prefix(root).unwrap_or(dir);
    if rel.as_os_str().is_empty() {
        "/".to_string()
    } else {
        format!("/{}", rel.display())
    }
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<CgroupMem>) {
    if depth > MAX_DEPTH {
        return;
    }
    if let Some(mut cg) = read_one(dir) {
        cg.path = rel_path(root, dir);
        out.push(cg);
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    children.sort();
    for child in children {
        walk(root, &child, depth + 1, out);
    }
}

/// All cgroups that expose the memory controller, biggest first.
/// Returns an empty vec on cgroup v1 or when the hierarchy is unreadable.
pub fn scan(root: &Path) -> Vec<CgroupMem> {
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out.sort_by_key(|c| std::cmp::Reverse(c.current_kb));
    out
}

/// True when this is a cgroup v2 hierarchy.
pub fn is_v2(root: &Path) -> bool {
    root.join("cgroup.controllers").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "\
anon 104857600
file 52428800
kernel 8388608
kernel_stack 1048576
sock 4096
shmem 2097152
slab 4194304
pgfault 12345
";

    #[test]
    fn parses_the_fields_we_display() {
        let cg = parse_stat(STAT, 209_715_200);
        assert_eq!(cg.current_kb, 204_800);
        assert_eq!(cg.anon_kb, 102_400);
        assert_eq!(cg.file_kb, 51_200);
        assert_eq!(cg.kernel_kb, 8_192);
        assert_eq!(cg.slab_kb, 4_096);
        assert_eq!(cg.shmem_kb, 2_048);
    }

    #[test]
    fn unknown_keys_and_garbage_lines_are_ignored() {
        let cg = parse_stat("anon 1024\nnot-a-pair\nfile nonsense\n", 2048);
        assert_eq!(cg.anon_kb, 1);
        assert_eq!(cg.file_kb, 0, "unparseable value must not panic or count");
    }

    #[test]
    fn limit_handles_the_max_literal() {
        assert_eq!(parse_limit("max\n"), None);
        assert_eq!(parse_limit("1073741824\n"), Some(1_048_576));
        assert_eq!(parse_limit("garbage"), None);
    }

    #[test]
    fn oom_kills_are_read_from_events() {
        let events = "low 0\nhigh 0\nmax 3\noom 1\noom_kill 7\noom_group_kill 0\n";
        assert_eq!(parse_oom_kills(events), 7);
        assert_eq!(parse_oom_kills(""), 0);
    }
}
