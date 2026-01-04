use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeEstimateMethod {
    Du,
    WalkDir,
    WalkDirTruncated,
    BudgetExhausted,
}

#[derive(Debug, Clone, Copy)]
pub struct SizeEstimate {
    pub bytes: u64,
    pub file_count: u64,
    pub error_count: u64,
    pub method: SizeEstimateMethod,
}

impl SizeEstimate {
    pub fn confidence(self) -> f64 {
        match self.method {
            SizeEstimateMethod::Du => 0.8,
            SizeEstimateMethod::WalkDir => {
                if self.file_count == 0 {
                    0.5
                } else if self.error_count == 0 {
                    0.9
                } else {
                    0.5
                }
            }
            SizeEstimateMethod::WalkDirTruncated => 0.3,
            SizeEstimateMethod::BudgetExhausted => 0.3,
        }
    }
}

pub fn estimate_dir_size(
    path: &Path,
    max_duration: Duration,
    deadline: Option<Instant>,
) -> Result<SizeEstimate> {
    let mut end = Instant::now() + max_duration;
    if let Some(d) = deadline {
        if d < end {
            end = d;
        }
    }
    if Instant::now() >= end {
        return Ok(SizeEstimate {
            bytes: 0,
            file_count: 0,
            error_count: 1,
            method: SizeEstimateMethod::BudgetExhausted,
        });
    }

    if let Some(estimate) = estimate_dir_size_du(path, end) {
        return Ok(estimate);
    }

    Ok(estimate_dir_size_walkdir(path, end))
}

fn estimate_dir_size_du(path: &Path, end: Instant) -> Option<SizeEstimate> {
    let timeout = end.saturating_duration_since(Instant::now());
    if timeout == Duration::from_secs(0) {
        return None;
    }

    let path_s = path.display().to_string();
    let args = vec!["-sk", path_s.as_str()];
    let out = crate::platform::run_command("du", &args, timeout).ok()?;
    if out.exit_code != 0 {
        return None;
    }

    let kb = out
        .stdout
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<u64>().ok())?;

    Some(SizeEstimate {
        bytes: kb.saturating_mul(1024),
        file_count: 0,
        error_count: 0,
        method: SizeEstimateMethod::Du,
    })
}

fn estimate_dir_size_walkdir(path: &Path, end: Instant) -> SizeEstimate {
    let mut bytes: u64 = 0;
    let mut files: u64 = 0;
    let mut errors: u64 = 0;
    let mut truncated = false;

    for entry in WalkDir::new(path).follow_links(false).into_iter() {
        if Instant::now() >= end {
            truncated = true;
            break;
        }
        match entry {
            Ok(entry) => {
                let ft = entry.file_type();
                if !ft.is_file() {
                    continue;
                }
                let meta = entry
                    .metadata()
                    .with_context(|| format!("メタデータ取得: {}", entry.path().display()));
                match meta {
                    Ok(meta) => {
                        bytes = bytes.saturating_add(meta.len());
                        files = files.saturating_add(1);
                    }
                    Err(_) => {
                        errors = errors.saturating_add(1);
                    }
                }
            }
            Err(_) => {
                errors = errors.saturating_add(1);
            }
        }
    }

    if truncated {
        errors = errors.max(1);
    }

    SizeEstimate {
        bytes,
        file_count: files,
        error_count: errors,
        method: if truncated {
            SizeEstimateMethod::WalkDirTruncated
        } else {
            SizeEstimateMethod::WalkDir
        },
    }
}

#[derive(Debug, Clone)]
pub struct TopDirEntry {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct TopDirsResult {
    pub root: PathBuf,
    pub total_bytes: u64,
    pub file_count: u64,
    pub error_count: u64,
    pub entries: Vec<TopDirEntry>,
}

pub fn top_directories(
    root: &Path,
    max_depth: usize,
    top_n: usize,
    excludes: &[String],
) -> Result<TopDirsResult> {
    let mut buckets: HashMap<PathBuf, u64> = HashMap::new();
    let mut total_bytes: u64 = 0;
    let mut file_count: u64 = 0;
    let mut error_count: u64 = 0;

    let max_depth = max_depth.max(1);
    let exclude_set = build_exclude_set(excludes)?;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !exclude_set.is_match(e.path()));

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                error_count = error_count.saturating_add(1);
                continue;
            }
        };

        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }

        let meta = entry
            .metadata()
            .with_context(|| format!("メタデータ取得: {}", path.display()));
        let meta = match meta {
            Ok(meta) => meta,
            Err(_) => {
                error_count = error_count.saturating_add(1);
                continue;
            }
        };

        let bytes = meta.len();
        total_bytes = total_bytes.saturating_add(bytes);
        file_count = file_count.saturating_add(1);

        if let Some(bucket) = bucket_dir(path, root, max_depth) {
            let acc = buckets.entry(bucket).or_insert(0);
            *acc = acc.saturating_add(bytes);
        }
    }

    let mut entries: Vec<TopDirEntry> = buckets
        .into_iter()
        .map(|(path, bytes)| TopDirEntry { path, bytes })
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.bytes));
    entries.truncate(top_n.max(1));

    Ok(TopDirsResult {
        root: root.to_path_buf(),
        total_bytes,
        file_count,
        error_count,
        entries,
    })
}

pub fn validate_excludes(excludes: &[String]) -> Result<()> {
    let _ = build_exclude_set(excludes)?;
    Ok(())
}

fn build_exclude_set(excludes: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pat in [
        "**/node_modules",
        "**/node_modules/**",
        "**/.git",
        "**/.git/**",
        "**/target",
        "**/target/**",
    ] {
        builder.add(Glob::new(pat).with_context(|| format!("exclude glob が不正です: {pat}"))?);
    }
    for pat in excludes {
        builder.add(Glob::new(pat).with_context(|| format!("exclude glob が不正です: {pat}"))?);
    }
    Ok(builder.build()?)
}

fn bucket_dir(file_path: &Path, root: &Path, max_depth: usize) -> Option<PathBuf> {
    let parent = file_path.parent()?;
    let rel = parent.strip_prefix(root).ok()?;

    let mut bucket = root.to_path_buf();
    for component in rel.components().take(max_depth) {
        bucket.push(component.as_os_str());
    }

    Some(bucket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn estimate_dir_size_respects_deadline_and_returns_budget_exhausted() {
        static HOME_SEQ: AtomicU64 = AtomicU64::new(0);

        let temp = std::env::temp_dir();
        let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = temp.join(format!(
            "macdiet-estimate-test-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(dir.join("file.bin"), b"hello").expect("write");

        let deadline = Instant::now() - Duration::from_secs(1);
        let est =
            estimate_dir_size(&dir, Duration::from_secs(5), Some(deadline)).expect("estimate");
        assert_eq!(est.bytes, 0);
        assert!(est.error_count > 0);
        assert_eq!(est.method, SizeEstimateMethod::BudgetExhausted);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn confidence_is_method_aware() {
        let du = SizeEstimate {
            bytes: 1,
            file_count: 0,
            error_count: 0,
            method: SizeEstimateMethod::Du,
        };
        assert!((du.confidence() - 0.8).abs() < 1e-9);

        let full = SizeEstimate {
            bytes: 1,
            file_count: 1,
            error_count: 0,
            method: SizeEstimateMethod::WalkDir,
        };
        assert!((full.confidence() - 0.9).abs() < 1e-9);

        let truncated = SizeEstimate {
            bytes: 1,
            file_count: 1,
            error_count: 1,
            method: SizeEstimateMethod::WalkDirTruncated,
        };
        assert!((truncated.confidence() - 0.3).abs() < 1e-9);
    }
}
