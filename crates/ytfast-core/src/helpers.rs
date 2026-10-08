//! The two helper programs YtFast runs, downloaded on first use:
//!
//! - **yt-dlp** finds each song's audio for the signed-in account. YouTube
//!   changes often and yt-dlp's team follows it, so the latest release is
//!   used, checked against the release's published SHA-256 list.
//! - **Deno** runs the JavaScript yt-dlp needs to answer YouTube's
//!   challenges. Its version is pinned and its fingerprint is written below.
//!
//! Both are the official GitHub release archives for this computer. They
//! are unpacked into YtFast's own data folder; nothing is installed system
//! wide.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};

/// The Deno release YtFast uses (yt-dlp needs 2.3 or later).
pub const DENO_VERSION: &str = "2.9.7";

/// What to download for one kind of computer.
#[derive(Clone, Copy, Debug)]
pub struct Platform {
    pub name: &'static str,
    /// yt-dlp's "onedir" archive: it starts faster than the single-file
    /// build, which unpacks itself on every run.
    yt_dlp_archive: &'static str,
    yt_dlp_exe: &'static str,
    deno_archive: &'static str,
    deno_exe: &'static str,
    /// SHA-256 of `deno_archive` for [`DENO_VERSION`], from Deno's release.
    deno_sha256: &'static str,
}

/// This computer's downloads, or `None` where YtFast has no plan yet.
pub fn platform() -> Option<Platform> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(Platform {
            name: "macOS (Apple Silicon)",
            yt_dlp_archive: "yt-dlp_macos.zip",
            yt_dlp_exe: "yt-dlp_macos",
            deno_archive: "deno-aarch64-apple-darwin.zip",
            deno_exe: "deno",
            deno_sha256: "5cd46d6268f6f78f5d88bdc7159d20bd44cdaa4b3303474839f87ec6fe7ae25c",
        })
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some(Platform {
            name: "macOS (Intel)",
            yt_dlp_archive: "yt-dlp_macos.zip",
            yt_dlp_exe: "yt-dlp_macos",
            deno_archive: "deno-x86_64-apple-darwin.zip",
            deno_exe: "deno",
            deno_sha256: "95daaff11c116a52ad54785e7914c8e9c9cdcaba793c5ed929c74ca2d8e6259a",
        })
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some(Platform {
            name: "Windows (Intel/AMD)",
            yt_dlp_archive: "yt-dlp_win.zip",
            yt_dlp_exe: "yt-dlp.exe",
            deno_archive: "deno-x86_64-pc-windows-msvc.zip",
            deno_exe: "deno.exe",
            deno_sha256: "a0c3101b4158d1dfb7d6a78a7bf0f3de80c96bb423c152beec8beb22786f2238",
        })
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(Platform {
            name: "Linux (x86-64)",
            yt_dlp_archive: "yt-dlp_linux.zip",
            yt_dlp_exe: "yt-dlp_linux",
            deno_archive: "deno-x86_64-unknown-linux-gnu.zip",
            deno_exe: "deno",
            deno_sha256: "c6527f24f4b16031d3ae4fa9f658d5f11534c8d84ce7dc8502420280919c3490",
        })
    } else {
        None
    }
}

/// The installed helpers.
#[derive(Clone, Debug)]
pub struct Helpers {
    pub yt_dlp: PathBuf,
    pub yt_dlp_version: String,
    pub deno: PathBuf,
    pub deno_version: String,
}

/// What [`ensure`] is doing, for a progress line.
#[derive(Clone, Debug)]
pub enum Progress {
    Checking,
    Downloading {
        what: &'static str,
        done: u64,
        total: Option<u64>,
    },
    Unpacking(&'static str),
    UsingInstalled {
        what: &'static str,
        version: String,
    },
}

/// Makes sure yt-dlp (the latest release) and Deno are in `dir`, and
/// returns where they are. When GitHub cannot be reached but a yt-dlp is
/// already installed, that one is used.
pub async fn ensure(
    http: &reqwest::Client,
    dir: &Path,
    progress: &(dyn Fn(Progress) + Sync),
) -> Result<Helpers> {
    let platform = platform()
        .ok_or_else(|| anyhow!("YtFast has no helper downloads for this kind of computer yet"))?;
    tokio::fs::create_dir_all(dir).await?;

    progress(Progress::Checking);
    let yt_dlp_parent = dir.join("yt-dlp");
    let installed = newest_installed(&yt_dlp_parent, platform.yt_dlp_exe);
    let yt_dlp_version = match latest_yt_dlp_version().await {
        Ok(latest) if usable(&yt_dlp_parent.join(&latest), platform.yt_dlp_exe) => {
            progress(Progress::UsingInstalled {
                what: "yt-dlp",
                version: latest.clone(),
            });
            latest
        }
        Ok(latest) => match install_yt_dlp(http, platform, &yt_dlp_parent, &latest, progress).await
        {
            Ok(()) => {
                remove_old_versions(&yt_dlp_parent, &latest);
                latest
            }
            // The newest could not be fetched: the one installed still
            // works, and the next start tries again.
            Err(error) => match installed {
                Some(installed) => {
                    log::warn!(
                        "yt-dlp {latest} could not be installed, so {installed} is used: {error:#}"
                    );
                    installed
                }
                None => return Err(error),
            },
        },
        Err(error) => match installed {
            Some(installed) => installed,
            None => return Err(error.context("could not ask GitHub for the latest yt-dlp")),
        },
    };
    let yt_dlp_dir = yt_dlp_parent.join(&yt_dlp_version);

    let deno_dir = dir.join("deno").join(DENO_VERSION);
    if usable(&deno_dir, platform.deno_exe) {
        progress(Progress::UsingInstalled {
            what: "Deno",
            version: DENO_VERSION.into(),
        });
    } else {
        let url = format!(
            "https://github.com/denoland/deno/releases/download/v{DENO_VERSION}/{}",
            platform.deno_archive
        );
        install(
            http,
            &url,
            platform.deno_sha256,
            &deno_dir,
            "Deno",
            progress,
        )
        .await?;
        remove_old_versions(&dir.join("deno"), DENO_VERSION);
    }

    let yt_dlp = yt_dlp_dir.join(platform.yt_dlp_exe);
    let deno = deno_dir.join(platform.deno_exe);
    for exe in [&yt_dlp, &deno] {
        if !exe.is_file() {
            bail!("{} is missing after unpacking", exe.display());
        }
    }
    Ok(Helpers {
        yt_dlp,
        yt_dlp_version,
        deno,
        deno_version: DENO_VERSION.into(),
    })
}

/// The release GitHub's "latest download" link redirects to. This avoids
/// the API (which has a low anonymous limit) and the release web page
/// (which some networks block while allowing downloads).
async fn latest_yt_dlp_version() -> Result<String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(crate::net::USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let response = client
        .get("https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS")
        .send()
        .await?;
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|l| l.to_str().ok())
        .ok_or_else(|| anyhow!("GitHub did not redirect to a release"))?;
    version_from_release_url(location).ok_or_else(|| anyhow!("unexpected release address"))
}

/// The release in `.../releases/download/<release>/<file>` or
/// `.../releases/tag/<release>`.
fn version_from_release_url(location: &str) -> Option<String> {
    let after = location
        .split_once("/releases/download/")
        .or_else(|| location.split_once("/releases/tag/"))?
        .1;
    let tag = after.split('/').next()?;
    // It becomes a folder name: letters, digits and single dots only.
    let valid = tag
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric())
        && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
        && !tag.contains("..");
    valid.then(|| tag.to_string())
}

/// The SHA-256 for `file` in a checksum list. Accepts `sha256sum` lines
/// (`<hash>  <name>`) and Windows' `Get-FileHash` tables (`Hash : <HASH>`)
/// when they list one file.
pub fn checksum_for(list: &str, file: &str) -> Option<String> {
    let is_hash = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    for line in list.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(hash), Some(name)) = (parts.next(), parts.next())
            && is_hash(hash)
            && name.trim_start_matches('*') == file
        {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

/// A finished install has this marker, written last.
const COMPLETE: &str = ".ytfast-complete";

fn is_complete(dir: &Path) -> bool {
    dir.join(COMPLETE).is_file()
}

/// Whether a helper's folder holds a whole install: unpacked to the end,
/// and its program still there (a virus scanner may take it away; it is
/// then downloaded again).
fn usable(dir: &Path, exe: &str) -> bool {
    is_complete(dir) && dir.join(exe).is_file()
}

/// The newest usable version in `parent`, whose program is `exe`.
fn newest_installed(parent: &Path, exe: &str) -> Option<String> {
    let mut versions: Vec<String> = std::fs::read_dir(parent)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| usable(&e.path(), exe))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    versions.sort();
    versions.pop()
}

/// Downloads yt-dlp `version` into `parent`, checked against the release's
/// published checksums.
async fn install_yt_dlp(
    http: &reqwest::Client,
    platform: Platform,
    parent: &Path,
    version: &str,
    progress: &(dyn Fn(Progress) + Sync),
) -> Result<()> {
    let base = format!("https://github.com/yt-dlp/yt-dlp/releases/download/{version}");
    let sums = http
        .get(format!("{base}/SHA2-256SUMS"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .context("could not download yt-dlp's checksum list")?
        .text()
        .await?;
    let expected = checksum_for(&sums, platform.yt_dlp_archive).ok_or_else(|| {
        anyhow!(
            "yt-dlp's checksum list has no entry for {}",
            platform.yt_dlp_archive
        )
    })?;
    install(
        http,
        &format!("{base}/{}", platform.yt_dlp_archive),
        &expected,
        &parent.join(version),
        "yt-dlp",
        progress,
    )
    .await
}

/// Keeps `keep` and the newest other complete version (to go back to if a
/// new yt-dlp misbehaves); removes the rest.
fn remove_old_versions(parent: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    let mut others: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n != keep))
        .collect();
    others.sort();
    let previous = others.iter().rev().find(|p| is_complete(p)).cloned();
    for path in others {
        if Some(&path) != previous.as_ref() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// Downloads `url`, checks it against `sha256`, and unpacks it into `dest`.
async fn install(
    http: &reqwest::Client,
    url: &str,
    sha256: &str,
    dest: &Path,
    what: &'static str,
    progress: &(dyn Fn(Progress) + Sync),
) -> Result<()> {
    let parent = dest.parent().ok_or_else(|| anyhow!("no parent folder"))?;
    tokio::fs::create_dir_all(parent).await?;
    let work = tempfile::Builder::new()
        .prefix(".download-")
        .tempdir_in(parent)?;
    let archive = work.path().join("archive.zip");
    download(http, url, &archive, what, progress).await?;

    progress(Progress::Unpacking(what));
    let (sha256, dest) = (sha256.to_string(), dest.to_path_buf());
    tokio::task::spawn_blocking(move || install_archive(&archive, &sha256, &dest, what)).await?
}

/// Streams `url` to `path`.
async fn download(
    http: &reqwest::Client,
    url: &str,
    path: &Path,
    what: &'static str,
    progress: &(dyn Fn(Progress) + Sync),
) -> Result<()> {
    let mut response = http
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("could not download {what}"))?;
    let total = response.content_length();
    let mut file = std::fs::File::create(path)?;
    let mut done = 0u64;
    let mut shown = 0u64;
    progress(Progress::Downloading { what, done, total });
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("the {what} download was interrupted"))?
    {
        file.write_all(&chunk)?;
        done += chunk.len() as u64;
        // A progress update per megabyte is plenty.
        if done - shown >= 1024 * 1024 {
            shown = done;
            progress(Progress::Downloading { what, done, total });
        }
    }
    progress(Progress::Downloading { what, done, total });
    file.sync_all()?;
    Ok(())
}

/// Checks the downloaded `archive` against its published `sha256`, then
/// unpacks it into `dest` and marks it complete. When the fingerprint does
/// not match, nothing is unpacked and `dest` is left as it was.
fn install_archive(archive: &Path, sha256: &str, dest: &Path, what: &str) -> Result<()> {
    let actual = sha256_of(archive)?;
    if !actual.eq_ignore_ascii_case(sha256) {
        bail!("the {what} download did not match its published fingerprint, so it was not used");
    }
    let parent = dest.parent().ok_or_else(|| anyhow!("no parent folder"))?;
    let work = tempfile::Builder::new()
        .prefix(".unpack-")
        .tempdir_in(parent)?;
    let unpacked = work.path().join("unpacked");
    unzip(archive, &unpacked)?;
    std::fs::write(unpacked.join(COMPLETE), &actual)?;

    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::rename(&unpacked, dest)
        .with_context(|| format!("could not move {what} into {}", dest.display()))?;
    Ok(())
}

/// A file's SHA-256, in lowercase hex.
fn sha256_of(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Unpacks a zip archive. Entries that would land outside `dest` are
/// refused. Unix permissions in the archive are kept, so programs stay
/// runnable on macOS and Linux.
pub fn unzip(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).context("the download is not a valid archive")?;
    std::fs::create_dir_all(dest)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("the archive has an unsafe path: {}", entry.name()))?;
        let out = dest.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut target = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut target)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let executable = entry.unix_mode().is_some_and(|m| m & 0o111 != 0);
            let mode = if executable { 0o755 } else { 0o644 };
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_checksum_formats() {
        let sums = "1fa6733c37ea6fb51c99ad8fe785e7b7e5f3246c9b980230329d4fb72ed8d4d6  yt-dlp\n\
                    07e54b0865303c864006925913bce2604f8ee8cc6f18699bac9c309f9328a6d8  yt-dlp_macos.zip\n\
                    30b4c14aafab6082becff7881e41b76df46dc43ea7633479410a91e29da492bf *yt-dlp_win.zip\n";
        assert_eq!(
            checksum_for(sums, "yt-dlp_macos.zip").as_deref(),
            Some("07e54b0865303c864006925913bce2604f8ee8cc6f18699bac9c309f9328a6d8")
        );
        assert_eq!(
            checksum_for(sums, "yt-dlp_win.zip").as_deref(),
            Some("30b4c14aafab6082becff7881e41b76df46dc43ea7633479410a91e29da492bf")
        );
        // A name that only starts the same is not a match.
        assert_eq!(checksum_for(sums, "yt-dlp_macos"), None);
    }

    #[test]
    fn reads_the_release_tag() {
        assert_eq!(
            version_from_release_url(
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/SHA2-256SUMS"
            )
            .as_deref(),
            Some("2026.08.19")
        );
        assert_eq!(
            version_from_release_url("https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19")
                .as_deref(),
            Some("2026.08.19")
        );
        assert_eq!(
            version_from_release_url("https://github.com/yt-dlp/yt-dlp/releases"),
            None
        );
        assert_eq!(
            version_from_release_url("https://x/releases/tag/../../etc"),
            None
        );
    }

    #[test]
    fn unzips_and_refuses_unsafe_paths() {
        use zip::write::SimpleFileOptions;
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&good).unwrap());
            let exec = SimpleFileOptions::default().unix_permissions(0o755);
            zip.add_directory("_internal/", SimpleFileOptions::default())
                .unwrap();
            zip.start_file("_internal/lib.so", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"library").unwrap();
            zip.start_file("tool", exec).unwrap();
            zip.write_all(b"#!/bin/sh\n").unwrap();
            zip.finish().unwrap();
        }
        let out = dir.path().join("out");
        unzip(&good, &out).unwrap();
        assert_eq!(
            std::fs::read(out.join("_internal/lib.so")).unwrap(),
            b"library"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(out.join("tool"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }

        let bad = dir.path().join("bad.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&bad).unwrap());
            zip.start_file("../escaped", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }
        assert!(unzip(&bad, &dir.path().join("out2")).is_err());
        assert!(!dir.path().join("escaped").exists());
    }

    #[test]
    fn installs_only_a_download_that_matches_its_fingerprint() {
        use zip::write::SimpleFileOptions;
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("archive.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            zip.start_file("deno.exe", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"the program").unwrap();
            zip.finish().unwrap();
        }
        let right = sha256_of(&archive).unwrap();
        assert_eq!(right.len(), 64);
        {
            // Checked against an independent hash of the same bytes.
            let bytes = std::fs::read(&archive).unwrap();
            let expected: String = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            assert_eq!(right, expected);
        }
        let dest = dir.path().join("deno").join("2.9.7");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();

        // Another file's fingerprint: refused, and nothing is installed.
        let wrong = "0".repeat(64);
        assert_ne!(wrong, right);
        let refused = install_archive(&archive, &wrong, &dest, "Deno");
        assert!(refused.is_err());
        assert!(!dest.exists());
        assert!(!is_complete(&dest));

        // The right one, as published in capitals: installed and marked
        // complete.
        install_archive(&archive, &right.to_ascii_uppercase(), &dest, "Deno").unwrap();
        assert_eq!(
            std::fs::read(dest.join("deno.exe")).unwrap(),
            b"the program"
        );
        assert!(usable(&dest, "deno.exe"));
        // Nothing is left behind beside it.
        let left: Vec<String> = std::fs::read_dir(dest.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(left, ["2.9.7"]);
    }

    #[test]
    fn keeps_the_current_and_previous_versions() {
        let dir = tempfile::tempdir().unwrap();
        for v in ["2026.06.09", "2026.07.04", "2026.08.19"] {
            let p = dir.path().join(v);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join(COMPLETE), "x").unwrap();
            std::fs::write(p.join("yt-dlp.exe"), "x").unwrap();
        }
        std::fs::create_dir_all(dir.path().join(".download-leftover")).unwrap();
        assert_eq!(
            newest_installed(dir.path(), "yt-dlp.exe").as_deref(),
            Some("2026.08.19")
        );
        // A version whose program went missing (a virus scanner took it)
        // is not used: it is downloaded again.
        let program = dir.path().join("2026.08.19").join("yt-dlp.exe");
        std::fs::remove_file(&program).unwrap();
        assert!(!usable(&dir.path().join("2026.08.19"), "yt-dlp.exe"));
        assert_eq!(
            newest_installed(dir.path(), "yt-dlp.exe").as_deref(),
            Some("2026.07.04")
        );
        std::fs::write(&program, "x").unwrap();
        remove_old_versions(dir.path(), "2026.08.19");
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["2026.07.04", "2026.08.19"]);
    }
}
