//! Chrome binary patcher
//!
//! Patches Chrome/Chromium binary to disable automation detection.
//! Uses Aho-Corasick for O(n) multi-pattern matching.

use aho_corasick::{AhoCorasick, Match};
use memmap2::MmapMut;
use rand::Rng;
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::error::{Error, Result};

// Thread-local RNG
thread_local! {
    static RNG: RefCell<rand::rngs::ThreadRng> = RefCell::new(rand::rng());
}

/// Patch pattern with replacement strategy
#[derive(Clone)]
struct PatchPattern {
    pattern: &'static [u8],
    strategy: PatchStrategy,
}

#[derive(Clone, Copy, PartialEq)]
enum PatchStrategy {
    RandomizePrefix,
    Scramble,
    Nullify,
    Skip,
}

/// All patterns to find and patch
static PATCH_PATTERNS: &[PatchPattern] = &[
    PatchPattern {
        pattern: b"$cdc_",
        strategy: PatchStrategy::RandomizePrefix,
    },
    PatchPattern {
        pattern: b"cdc_",
        strategy: PatchStrategy::RandomizePrefix,
    },
    PatchPattern {
        pattern: b"webdriver",
        strategy: PatchStrategy::Scramble,
    },
    PatchPattern {
        pattern: b"--enable-automation",
        strategy: PatchStrategy::Nullify,
    },
    PatchPattern {
        pattern: b"devtoolsw",
        strategy: PatchStrategy::Scramble,
    },
    PatchPattern {
        pattern: b"debuggerPrivate",
        strategy: PatchStrategy::Scramble,
    },
    PatchPattern {
        pattern: b"HeadlessChrome",
        strategy: PatchStrategy::Scramble,
    },
    PatchPattern {
        pattern: b"$wdc_",
        strategy: PatchStrategy::RandomizePrefix,
    },
    PatchPattern {
        pattern: b"$chromeDriver",
        strategy: PatchStrategy::RandomizePrefix,
    },
    // Detection only
    PatchPattern {
        pattern: b"Runtime.enable",
        strategy: PatchStrategy::Skip,
    },
    PatchPattern {
        pattern: b"Page.addScriptToEvaluateOnNewDocument",
        strategy: PatchStrategy::Skip,
    },
];

/// Compiled pattern matcher (lazy initialized)
static PATTERN_MATCHER: OnceLock<AhoCorasick> = OnceLock::new();

fn get_pattern_matcher() -> &'static AhoCorasick {
    PATTERN_MATCHER.get_or_init(|| {
        let patterns: Vec<&[u8]> = PATCH_PATTERNS.iter().map(|p| p.pattern).collect();
        AhoCorasick::new(&patterns).expect("Failed to build Aho-Corasick automaton")
    })
}

/// Find Chrome binary on the system
pub fn find_chrome() -> Result<PathBuf> {
    let candidates = if cfg!(target_os = "macos") {
        vec![
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        ]
    } else if cfg!(target_os = "linux") {
        vec![
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/snap/bin/chromium",
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ]
    } else {
        vec![]
    };

    for candidate in candidates {
        let path = Path::new(candidate);
        if path.exists() {
            #[cfg(target_os = "linux")]
            {
                let resolved = resolve_to_elf(path);
                return Ok(resolved);
            }
            #[cfg(not(target_os = "linux"))]
            return Ok(path.to_path_buf());
        }
    }

    Err(Error::ChromeNotFound)
}

/// On Linux, `/usr/bin/chromium-browser` (and similar) are often shell script
/// wrappers, not actual ELF binaries. The patcher needs the real binary so it
/// can rewrite detection markers. This function checks the magic bytes and, if
/// the candidate is not an ELF, looks for the real binary in the standard lib
/// directories where distros place it.
#[cfg(target_os = "linux")]
fn resolve_to_elf(path: &Path) -> PathBuf {
    if is_elf(path) {
        return path.to_path_buf();
    }

    // Follow symlinks first — google-chrome might be a symlink to the real thing
    if let Ok(resolved) = fs::canonicalize(path) {
        if is_elf(&resolved) {
            tracing::info!("Resolved symlink {:?} to ELF binary {:?}", path, resolved);
            return resolved;
        }
    }

    // Derive the binary name from the candidate (e.g. "chromium-browser")
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("chromium-browser");

    // Standard lib directories where distros keep the actual ELF
    let lib_candidates = [
        format!("/usr/lib/{0}/{0}", name),
        format!("/usr/lib64/{0}/{0}", name),
        "/usr/lib/chromium-browser/chromium-browser".to_string(),
        "/usr/lib/chromium/chromium".to_string(),
        "/opt/google/chrome/chrome".to_string(),
        "/opt/google/chrome/google-chrome".to_string(),
    ];

    for lib_path in &lib_candidates {
        let p = Path::new(lib_path);
        if p.exists() && is_elf(p) {
            tracing::info!("Resolved wrapper script {:?} to ELF binary {:?}", path, p);
            return p.to_path_buf();
        }
    }

    // Fallback: return as-is and let the patcher fail with a clear error
    tracing::warn!(
        "Chrome candidate {:?} is not an ELF binary and no lib binary was found",
        path,
    );
    path.to_path_buf()
}

/// Check if a file starts with the ELF magic bytes (0x7f ELF).
#[cfg(target_os = "linux")]
fn is_elf(path: &Path) -> bool {
    File::open(path)
        .and_then(|mut f| {
            let mut magic = [0u8; 4];
            f.read_exact(&mut magic)?;
            Ok(magic == [0x7f, b'E', b'L', b'F'])
        })
        .unwrap_or(false)
}

/// Chrome binary patcher
pub struct ChromePatcher {
    original_path: PathBuf,
    patched_path: PathBuf,
    #[cfg(target_os = "macos")]
    original_bundle: Option<PathBuf>,
    #[cfg(target_os = "macos")]
    patched_bundle: Option<PathBuf>,
}

impl ChromePatcher {
    /// Create a new patcher for the given Chrome binary
    pub fn new(chrome_path: &Path) -> Result<Self> {
        if !chrome_path.exists() {
            return Err(Error::patching(
                "new",
                format!("Chrome binary not found: {:?}", chrome_path),
            ));
        }

        #[cfg(target_os = "macos")]
        {
            // On macOS, handle the full .app bundle
            let mut bundle_path: Option<PathBuf> = None;
            let mut current = chrome_path.to_path_buf();
            while let Some(parent) = current.parent() {
                if current.extension().map(|e| e == "app").unwrap_or(false) {
                    bundle_path = Some(current.clone());
                    break;
                }
                current = parent.to_path_buf();
            }

            if let Some(bundle) = bundle_path {
                let bundle_name = bundle
                    .file_name()
                    .ok_or_else(|| Error::patching("new", "Invalid bundle path"))?;

                let patched_bundle = std::env::temp_dir().join("eoka-chrome").join(bundle_name);

                let relative_path = chrome_path
                    .strip_prefix(&bundle)
                    .map_err(|_| Error::patching("new", "Binary not inside bundle"))?;
                let patched_path = patched_bundle.join(relative_path);

                return Ok(Self {
                    original_path: chrome_path.to_path_buf(),
                    patched_path,
                    original_bundle: Some(bundle),
                    patched_bundle: Some(patched_bundle),
                });
            }
        }

        // Non-macOS or non-bundled binary
        let filename = chrome_path
            .file_name()
            .ok_or_else(|| Error::patching("new", "Invalid path"))?;

        let patched_path = std::env::temp_dir().join("eoka-chrome").join(filename);

        Ok(Self {
            original_path: chrome_path.to_path_buf(),
            patched_path,
            #[cfg(target_os = "macos")]
            original_bundle: None,
            #[cfg(target_os = "macos")]
            patched_bundle: None,
        })
    }

    /// Check if a patched binary already exists and is valid
    pub fn is_patched(&self) -> bool {
        if !self.patched_path.exists() {
            return false;
        }

        let orig_modified = fs::metadata(&self.original_path)
            .and_then(|m| m.modified())
            .ok();
        let patched_modified = fs::metadata(&self.patched_path)
            .and_then(|m| m.modified())
            .ok();

        match (orig_modified, patched_modified) {
            (Some(orig), Some(patched)) if patched > orig => self.verify_patched_sample(),
            _ => false,
        }
    }

    /// Quick verification by checking first 64KB for unpatched patterns
    fn verify_patched_sample(&self) -> bool {
        const SAMPLE_SIZE: usize = 64 * 1024;

        let mut file = match File::open(&self.patched_path) {
            Ok(f) => f,
            Err(_) => return false,
        };

        let mut buffer = vec![0u8; SAMPLE_SIZE];
        let bytes_read = match file.read(&mut buffer) {
            Ok(n) => n,
            Err(_) => return false,
        };
        buffer.truncate(bytes_read);

        !get_pattern_matcher().is_match(&buffer)
    }

    /// Copy the app bundle (macOS) using symlinks for speed
    #[cfg(target_os = "macos")]
    fn copy_bundle(&self) -> Result<()> {
        let (orig_bundle, dest_bundle) = match (&self.original_bundle, &self.patched_bundle) {
            (Some(o), Some(d)) => (o, d),
            _ => return Ok(()),
        };

        if dest_bundle.exists() {
            fs::remove_dir_all(dest_bundle)?;
        }

        if let Some(parent) = dest_bundle.parent() {
            fs::create_dir_all(parent)?;
        }

        tracing::info!(
            "Creating Chrome bundle with symlinks at {:?}...",
            dest_bundle
        );
        self.copy_bundle_with_symlinks(orig_bundle, dest_bundle)?;

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn copy_bundle_with_symlinks(&self, src: &Path, dst: &Path) -> Result<()> {
        use std::os::unix::fs::symlink;

        fs::create_dir_all(dst)?;

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let file_name = entry.file_name();
            let dst_path = dst.join(&file_name);

            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                if self.original_path.starts_with(&src_path) {
                    self.copy_bundle_with_symlinks(&src_path, &dst_path)?;
                } else {
                    symlink(&src_path, &dst_path)?;
                }
            } else if file_type.is_file() {
                if src_path == self.original_path {
                    fs::copy(&src_path, &dst_path)?;
                    tracing::debug!("Copied binary for patching: {:?}", file_name);
                } else {
                    symlink(&src_path, &dst_path)?;
                }
            } else if file_type.is_symlink() {
                let target = fs::read_link(&src_path)?;
                symlink(&target, &dst_path)?;
            }
        }

        Ok(())
    }

    /// Symlink all sibling files from the original binary's directory into the
    /// patched directory. On Linux, Chromium uses `$ORIGIN` RPATH so it resolves
    /// shared libraries (libffmpeg.so, etc.) and data files (icudtl.dat, v8
    /// snapshots, locale paks) relative to the binary. Without these symlinks
    /// the patched copy in /tmp cannot find its resources and crashes.
    #[cfg(not(target_os = "macos"))]
    fn symlink_companion_files(&self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let orig_dir = match self.original_path.parent() {
                Some(d) => d,
                None => return,
            };
            let patch_dir = match self.patched_path.parent() {
                Some(d) => d,
                None => return,
            };

            // Nothing to do if source and destination are the same directory
            if orig_dir == patch_dir {
                return;
            }

            let orig_filename = self.original_path.file_name();

            let entries = match fs::read_dir(orig_dir) {
                Ok(e) => e,
                Err(err) => {
                    tracing::warn!("Could not read Chrome directory {:?}: {}", orig_dir, err);
                    return;
                }
            };

            for entry in entries.flatten() {
                // Skip the binary itself — we're patching that
                if Some(entry.file_name().as_os_str()) == orig_filename {
                    continue;
                }

                let dest = patch_dir.join(entry.file_name());
                if !dest.exists() {
                    if let Err(err) = symlink(entry.path(), &dest) {
                        tracing::trace!(
                            "Could not symlink {:?} -> {:?}: {}",
                            entry.path(),
                            dest,
                            err
                        );
                    }
                }
            }

            tracing::debug!(
                "Symlinked companion files from {:?} into {:?}",
                orig_dir,
                patch_dir
            );
        }
    }

    /// Get path to patched binary (patches if needed)
    pub fn get_patched_path(&self) -> Result<PathBuf> {
        if !self.is_patched() {
            self.patch()?;
        }
        Ok(self.patched_path.clone())
    }

    /// Perform the patching
    pub fn patch(&self) -> Result<()> {
        tracing::info!("Patching Chrome binary: {:?}", self.original_path);

        #[cfg(target_os = "macos")]
        {
            self.copy_bundle()?;
        }

        #[cfg(not(target_os = "macos"))]
        {
            if let Some(parent) = self.patched_path.parent() {
                fs::create_dir_all(parent)?;
            }
            // Symlink companion files (.so, .pak, .dat, .bin, locales, etc.)
            // so the patched binary can find its resources. Chromium uses
            // $ORIGIN RPATH, meaning it resolves libraries relative to the
            // binary — without these symlinks the patched copy crashes.
            self.symlink_companion_files();
        }

        #[cfg(target_os = "macos")]
        let read_path = if self.patched_bundle.is_some() {
            &self.patched_path
        } else {
            &self.original_path
        };
        #[cfg(not(target_os = "macos"))]
        let read_path = &self.original_path;

        let file_size = fs::metadata(read_path)?.len() as usize;

        let patch_count = if file_size > 10 * 1024 * 1024 {
            tracing::debug!(
                "Using memory-mapped patching for {}MB file",
                file_size / 1024 / 1024
            );
            self.patch_with_mmap(read_path)?
        } else {
            tracing::debug!("Using in-memory patching for {}KB file", file_size / 1024);
            self.patch_in_memory(read_path)?
        };

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&self.patched_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&self.patched_path, perms)?;
        }

        // Re-sign on macOS
        #[cfg(target_os = "macos")]
        {
            self.codesign()?;
        }

        tracing::info!(
            "Patched {} occurrences, saved to {:?}",
            patch_count,
            self.patched_path
        );

        Ok(())
    }

    fn patch_with_mmap(&self, read_path: &Path) -> Result<usize> {
        #[cfg(target_os = "macos")]
        let should_patch_in_place = self.patched_bundle.is_some();
        #[cfg(not(target_os = "macos"))]
        let should_patch_in_place = false;

        if !should_patch_in_place {
            fs::copy(read_path, &self.patched_path)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.patched_path)?;

        // SAFETY: We just created/copied this file and hold it open exclusively.
        // No other process is writing to it concurrently.
        let mut mmap = unsafe { MmapMut::map_mut(&file)? };
        let patch_count = self.apply_patches(&mut mmap);

        mmap.flush()?;
        Ok(patch_count)
    }

    fn patch_in_memory(&self, read_path: &Path) -> Result<usize> {
        let mut file = File::open(read_path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        let original_len = data.len();
        let patch_count = self.apply_patches(&mut data);

        if data.len() != original_len {
            return Err(Error::patching(
                "patch_in_memory",
                format!(
                    "Binary size changed during patching: {} -> {}",
                    original_len,
                    data.len()
                ),
            ));
        }

        #[cfg(not(target_os = "macos"))]
        if let Some(parent) = self.patched_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut out_file = File::create(&self.patched_path)?;
        out_file.write_all(&data)?;

        Ok(patch_count)
    }

    fn apply_patches(&self, data: &mut [u8]) -> usize {
        let matches: Vec<Match> = get_pattern_matcher().find_iter(&*data).collect();
        let mut patch_count = 0;
        let mut patched_ranges: Vec<(usize, usize)> = Vec::new();

        for m in matches {
            let pattern_idx = m.pattern().as_usize();
            let pattern = &PATCH_PATTERNS[pattern_idx];
            let start = m.start();
            let end = m.end();

            if patched_ranges
                .iter()
                .any(|(ps, pe)| start < *pe && end > *ps)
            {
                continue;
            }

            match pattern.strategy {
                PatchStrategy::RandomizePrefix => {
                    let replacement = random_string(pattern.pattern.len());
                    data[start..end].copy_from_slice(replacement.as_bytes());
                    patch_count += 1;
                    patched_ranges.push((start, end));
                }
                PatchStrategy::Scramble => {
                    for i in (0..pattern.pattern.len() - 1).step_by(2) {
                        data.swap(start + i, start + i + 1);
                    }
                    patch_count += 1;
                    patched_ranges.push((start, end));
                }
                PatchStrategy::Nullify => {
                    for byte in &mut data[start..end] {
                        *byte = b' ';
                    }
                    patch_count += 1;
                    patched_ranges.push((start, end));
                }
                PatchStrategy::Skip => {
                    tracing::trace!(
                        "Found (not patching): {:?} at offset {}",
                        String::from_utf8_lossy(pattern.pattern),
                        start
                    );
                }
            }
        }

        if patch_count > 0 {
            tracing::debug!("Applied {} patches using Aho-Corasick", patch_count);
        }

        patch_count
    }

    #[cfg(target_os = "macos")]
    fn codesign(&self) -> Result<()> {
        use std::process::Command;

        let sign_path = self.patched_bundle.as_ref().unwrap_or(&self.patched_path);

        tracing::info!("Re-signing {:?} with ad-hoc signature...", sign_path);

        let sign_str = sign_path
            .to_str()
            .ok_or_else(|| Error::patching("codesign", "Invalid UTF-8 in sign path"))?;

        let _ = Command::new("codesign")
            .args(["--remove-signature", sign_str])
            .output();

        let output = Command::new("codesign")
            .args(["-s", "-", "-f", "--deep", "--no-strict", sign_str])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("codesign warning: {}", stderr);
        } else {
            tracing::info!("Successfully re-signed patched binary");
        }

        Ok(())
    }
}

fn random_string(len: usize) -> String {
    RNG.with(|rng| {
        let mut rng = rng.borrow_mut();
        (0..len)
            .map(|_| {
                let idx = rng.random_range(0..36u8);
                if idx < 10 {
                    (b'0' + idx) as char
                } else {
                    (b'a' + idx - 10) as char
                }
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_chrome() {
        if let Ok(path) = find_chrome() {
            println!("Found Chrome at: {:?}", path);
            assert!(path.exists());
        }
    }

    /// Verify that find_chrome returns an ELF binary, not a shell wrapper.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_find_chrome_returns_elf() {
        if let Ok(path) = find_chrome() {
            assert!(
                is_elf(&path),
                "find_chrome() returned {:?} which is not an ELF binary",
                path,
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_is_elf() {
        // /usr/bin/ls should be an ELF binary on any Linux
        let ls = Path::new("/usr/bin/ls");
        if ls.exists() {
            assert!(is_elf(ls), "/usr/bin/ls should be ELF");
        }

        // A non-existent path is not ELF
        assert!(!is_elf(Path::new("/nonexistent")));
    }

    #[test]
    fn test_random_string() {
        let s = random_string(10);
        assert_eq!(s.len(), 10);
        assert!(s.chars().all(|c| c.is_alphanumeric()));
    }

    #[test]
    fn test_aho_corasick_patterns() {
        let test_data = b"$cdc_test webdriver HeadlessChrome";
        let matches: Vec<_> = get_pattern_matcher().find_iter(test_data).collect();
        assert!(matches.len() >= 3);
    }
}
