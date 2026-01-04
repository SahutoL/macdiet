use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default)]
pub struct ApfsSnapshotCatalog {
    pub uuids: BTreeSet<String>,
    pub names: BTreeSet<String>,
    pub name_to_uuids: BTreeMap<String, BTreeSet<String>>,
}

pub fn is_uuid(s: &str) -> bool {
    let mut parts = s.split('-');
    let Some(p1) = parts.next() else { return false };
    let Some(p2) = parts.next() else { return false };
    let Some(p3) = parts.next() else { return false };
    let Some(p4) = parts.next() else { return false };
    let Some(p5) = parts.next() else { return false };
    if parts.next().is_some() {
        return false;
    }
    if p1.len() != 8 || p2.len() != 4 || p3.len() != 4 || p4.len() != 4 || p5.len() != 12 {
        return false;
    }
    [p1, p2, p3, p4, p5]
        .iter()
        .all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn extract_diskutil_snapshot_uuids(stdout: &str) -> BTreeSet<String> {
    parse_diskutil_apfs_list_snapshots(stdout).uuids
}

pub fn parse_diskutil_apfs_list_snapshots(stdout: &str) -> ApfsSnapshotCatalog {
    #[derive(Debug, Clone, Default)]
    struct Entry {
        tree_id: Option<String>,
        name: Option<String>,
        uuids: BTreeSet<String>,
    }

    fn flush(mut entry: Entry, out: &mut ApfsSnapshotCatalog) {
        if !entry.uuids.is_empty() {
            for uuid in &entry.uuids {
                out.uuids.insert(uuid.clone());
            }

            if let Some(name) = entry.name.take() {
                out.names.insert(name.clone());
                let set = out.name_to_uuids.entry(name).or_default();
                for uuid in &entry.uuids {
                    set.insert(uuid.clone());
                }
            }

            if let Some(tree) = entry.tree_id.take() {
                out.names.insert(tree.clone());
                let set = out.name_to_uuids.entry(tree).or_default();
                for uuid in &entry.uuids {
                    set.insert(uuid.clone());
                }
            }
        } else {
            if let Some(name) = entry.name.take() {
                out.names.insert(name);
            }
            if let Some(tree) = entry.tree_id.take() {
                out.names.insert(tree);
            }
        }
    }

    let mut out = ApfsSnapshotCatalog::default();
    let mut cur = Entry::default();

    for line in stdout.lines() {
        let trimmed0 = line.trim_start();
        let trimmed = trimmed0
            .strip_prefix('|')
            .map(str::trim_start)
            .unwrap_or(trimmed0);
        if let Some(rest) = trimmed.strip_prefix("+--") {
            flush(std::mem::take(&mut cur), &mut out);
            let tree = rest.trim().to_string();
            if is_uuid(&tree) {
                cur.uuids.insert(tree.to_ascii_lowercase());
            } else if !tree.is_empty() {
                cur.tree_id = Some(tree);
            }
            continue;
        }

        let lower = trimmed.to_ascii_lowercase();

        if lower.starts_with("name:") {
            if let Some((_, rest)) = trimmed.split_once(':') {
                let name = rest.trim().to_string();
                if !name.is_empty() {
                    cur.name = Some(name);
                }
            }
            continue;
        }

        if lower.contains("uuid") {
            if let Some(uuid) = first_uuid_in_line(trimmed) {
                cur.uuids.insert(uuid.to_ascii_lowercase());
            }
        }
    }

    flush(cur, &mut out);
    out
}

fn first_uuid_in_line(line: &str) -> Option<String> {
    for token in line.split(|c: char| !c.is_ascii_hexdigit() && c != '-') {
        if is_uuid(token) {
            return Some(token.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_uuid_accepts_basic_uuid() {
        assert!(is_uuid("01234567-89ab-cdef-0123-456789abcdef"));
        assert!(is_uuid("01234567-89AB-CDEF-0123-456789ABCDEF"));
    }

    #[test]
    fn is_uuid_rejects_non_uuid() {
        assert!(!is_uuid("not-a-uuid"));
        assert!(!is_uuid("01234567-89ab-cdef-0123-456789abcde")); // too short
        assert!(!is_uuid("0123456789ab-cdef-0123-456789abcdef")); // missing dash
    }

    #[test]
    fn extract_diskutil_snapshot_uuids_pulls_uuid_tokens_from_uuid_lines() {
        let sample = r#"
Snapshots for disk1s1 (2 found)
|
+-- 01234567-89AB-CDEF-0123-456789ABCDEF
    Name: com.apple.TimeMachine.2026-01-01-000000.local
    Snapshot UUID: 89abcdef-0123-4567-89ab-cdef01234567
"#;
        let cat = parse_diskutil_apfs_list_snapshots(sample);
        assert!(cat.uuids.contains("01234567-89ab-cdef-0123-456789abcdef"));
        assert!(cat.uuids.contains("89abcdef-0123-4567-89ab-cdef01234567"));
        assert!(
            cat.name_to_uuids
                .get("com.apple.TimeMachine.2026-01-01-000000.local")
                .is_some()
        );
    }

    #[test]
    fn parse_diskutil_handles_pipe_prefixed_tree_lines() {
        let sample = r#"
Snapshots for disk1s1 (1 found)
|
| +-- com.apple.TimeMachine.2026-01-01-000000.local
|     Snapshot UUID: 89abcdef-0123-4567-89ab-cdef01234567
"#;
        let cat = parse_diskutil_apfs_list_snapshots(sample);
        assert!(cat.uuids.contains("89abcdef-0123-4567-89ab-cdef01234567"));
        let uuids = cat
            .name_to_uuids
            .get("com.apple.TimeMachine.2026-01-01-000000.local")
            .expect("name mapping");
        assert!(uuids.contains("89abcdef-0123-4567-89ab-cdef01234567"));
    }
}
