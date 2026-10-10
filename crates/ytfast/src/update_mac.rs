//! Updates on a Mac, done by YTFast itself.
//!
//! fastframe-update installs a Mac app only when Apple has signed it (a
//! paid developer certificate), which YTFast's builds are not. So on a Mac
//! YTFast takes the same steps itself, as carefully:
//!
//! - the release's `checksums.txt` must carry the publisher's signature
//!   (the key this build carries, as on Windows), and `YTFast.dmg` must
//!   match its checksum there;
//! - the app is copied out of the disk image into YTFast's cache, signed
//!   with this Mac's own "YTFast Local Signing" when it has one (the one
//!   `packaging/macos/install.sh` makes: the Mac then keeps what it allowed
//!   YTFast, Full Disk Access among them), checked with `codesign`, and
//!   asked its `--version`;
//! - "Restart to update" starts this program again as a helper
//!   (`--finish-mac-update`), which waits for YTFast to close, moves the
//!   old app aside and the new one into its place, and opens it. The old
//!   one goes back if the move fails. The helper leaves a line for the
//!   next start to show ([`take_result`]).
//!
//! Downloaded by YTFast rather than a browser, the new app is not marked
//! as from the internet, so the Mac does not ask about it again.
//!
//! The steps compile everywhere (the tests run on every system); they are
//! only taken on a Mac.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// The Mac download on every release.
const DISK_IMAGE: &str = "YTFast.dmg";
/// This Mac's own signature, made by `packaging/macos/install.sh`.
const LOCAL_SIGNATURE: &str = "YTFast Local Signing";
/// The largest disk image accepted.
const MOST_BYTES: u64 = 200 * 1024 * 1024;
/// How long the helper waits for YTFast to close.
const CLOSE_WAIT: Duration = Duration::from_secs(30);

/// A new version, checked and ready to take the old one's place.
pub struct Staged {
    pub version: String,
    app: PathBuf,
}

/// Where updates are prepared: YTFast's cache, `update`.
fn folder() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "YtFast").map(|dirs| dirs.cache_dir().join("update"))
}

/// What the helper left for this start to say: that YTFast was updated,
/// or why it could not be. Said once.
pub fn take_result() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let file = folder()?.join("result.txt");
    let said = fs::read_to_string(&file).ok()?;
    let _ = fs::remove_file(&file);
    Some(said.trim().to_string()).filter(|s| !s.is_empty())
}

/// The app this program runs from, when YTFast can put a new version in
/// its place: a `.app` in a folder YTFast may write in (Applications, as
/// a rule). Why not, in words for the update window, otherwise.
pub fn bundle() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let elsewhere = "Move YTFast into Applications and open it from there: it can then update itself. Or download this version from its page.";
    let app = bundle_of(&exe).ok_or(elsewhere)?;
    // Opened from the disk image, or from Downloads without being moved:
    // the Mac runs it from a copy no one may write in.
    let shown = app.to_string_lossy();
    if shown.contains("/AppTranslocation/") || shown.starts_with("/Volumes/") {
        return Err(elsewhere.into());
    }
    let parent = app.parent().ok_or(elsewhere)?;
    let probe = parent.join(format!(".ytfast-update-{}", std::process::id()));
    fs::create_dir(&probe)
        .and_then(|()| fs::remove_dir(&probe))
        .map_err(|_| {
            format!(
                "YTFast may not write in {}, so it cannot update itself there. Download this version from its page.",
                parent.display()
            )
        })?;
    Ok(app)
}

/// The `.app` an executable sits in (`<name>.app/Contents/MacOS/<it>`).
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app")
        .then(|| app.to_path_buf())
}

/// Downloads `version`'s disk image from its release, checks it, and gets
/// the app in it ready (see the module's notes). `progress` hears the
/// bytes so far and in all.
pub fn prepare(
    client: &reqwest::blocking::Client,
    repository: &str,
    public_key: &str,
    version: &str,
    mut progress: impl FnMut(u64, u64),
) -> Result<Staged, String> {
    let folder = folder().ok_or("YTFast has no cache folder to prepare the update in")?;
    // Whatever an earlier try left.
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let base = format!("https://github.com/{repository}/releases/download/v{version}/");
    let manifest = fetch_small(client, &format!("{base}checksums.txt"))?;
    let signature = fetch_small(client, &format!("{base}checksums.txt.sig"))?;
    let manifest = checked_manifest(&manifest, &signature, public_key)?;
    let expected = digest_for(&manifest, DISK_IMAGE)
        .ok_or_else(|| format!("the release has no {DISK_IMAGE}"))?;
    let image = folder.join(DISK_IMAGE);
    let digest = fetch_to(
        client,
        &format!("{base}{DISK_IMAGE}"),
        &image,
        &mut progress,
    )?;
    if digest != expected {
        return Err(format!("{DISK_IMAGE} does not match its checksum"));
    }
    let app = folder.join("YTFast.app");
    unpack(&image, &folder.join("mounted"), &app)?;
    let _ = fs::remove_file(&image);
    sign(&app)?;
    let said = run(&app.join("Contents/MacOS/ytfast"), &["--version"])?;
    if said.trim() != format!("ytfast {version}") {
        return Err(format!(
            "the new app says it is {}, not {version}",
            said.trim()
        ));
    }
    Ok(Staged {
        version: version.to_string(),
        app,
    })
}

/// `checksums.txt` as text, once its signature (64 bytes, Ed25519) is
/// found to be the publisher's.
fn checked_manifest(manifest: &[u8], signature: &[u8], public_key: &str) -> Result<String, String> {
    let key = unhex(public_key.trim()).ok_or("the publisher key is not hexadecimal")?;
    if signature.len() != 64 {
        return Err("the release's signature is not one".into());
    }
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key)
        .verify(manifest, signature)
        .map_err(|_| "the release is not signed with YTFast's key".to_string())?;
    String::from_utf8(manifest.to_vec()).map_err(|_| "checksums.txt is not text".into())
}

/// A file's SHA-256 in `checksums.txt` (`sha256sum` lines: the digest, two
/// spaces, the name).
fn digest_for(manifest: &str, name: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        let (digest, file) = line.split_once("  ")?;
        (file.trim() == name).then(|| digest.trim().to_ascii_lowercase())
    })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A small file (a checksum list, a signature), whole.
fn fetch_small(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| format!("could not download the release's checksums: {}", plain(&e)))?;
    let mut bytes = Vec::new();
    response
        .take(1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// Downloads `url` into `to`, telling `progress`; its SHA-256.
fn fetch_to(
    client: &reqwest::blocking::Client,
    url: &str,
    to: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<String, String> {
    let mut response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| format!("could not download {DISK_IMAGE}: {}", plain(&e)))?;
    let total = response.content_length().unwrap_or(0);
    if total > MOST_BYTES {
        return Err(format!("{DISK_IMAGE} is larger than expected"));
    }
    let mut file = fs::File::create(to).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let read = response
            .read(&mut chunk)
            .map_err(|e| format!("the download of {DISK_IMAGE} broke off: {e}"))?;
        if read == 0 {
            break;
        }
        received += read as u64;
        if received > MOST_BYTES {
            return Err(format!("{DISK_IMAGE} is larger than expected"));
        }
        hasher.update(&chunk[..read]);
        file.write_all(&chunk[..read]).map_err(|e| e.to_string())?;
        progress(received, total.max(received));
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(hex(&hasher.finalize()))
}

/// A download's failure without its address.
fn plain(error: &reqwest::Error) -> String {
    ytfast_core::redact::urls(&error.to_string())
}

/// Copies the app out of the disk image `image` into `app`, the image
/// mounted at `mount` meanwhile (out of sight, read only).
fn unpack(image: &Path, mount: &Path, app: &Path) -> Result<(), String> {
    let image_text = image.to_string_lossy();
    let mount_text = mount.to_string_lossy();
    run(
        Path::new("/usr/bin/hdiutil"),
        &[
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
            &mount_text,
            &image_text,
        ],
    )?;
    let copied = run(
        Path::new("/usr/bin/ditto"),
        &[
            &mount.join("YTFast.app").to_string_lossy(),
            &app.to_string_lossy(),
        ],
    );
    let detached = run(
        Path::new("/usr/bin/hdiutil"),
        &["detach", "-force", &mount_text],
    );
    if let Err(e) = detached {
        log::warn!("updates: the disk image did not detach: {e}");
    }
    copied.map(|_| ())
}

/// Signs the new app with this Mac's own signature, when it has one (or
/// leaves the build's), and checks the signature holds.
fn sign(app: &Path) -> Result<(), String> {
    let app_text = app.to_string_lossy();
    let has_own = run(
        Path::new("/usr/bin/security"),
        &["find-certificate", "-c", LOCAL_SIGNATURE],
    )
    .is_ok();
    if has_own
        && let Err(e) = run(
            Path::new("/usr/bin/codesign"),
            &["--force", "--deep", "--sign", LOCAL_SIGNATURE, &app_text],
        )
    {
        // The build's own signature still holds; the Mac may then ask for
        // its permissions again.
        log::warn!("updates: could not sign with this Mac's own signature: {e}");
    }
    run(
        Path::new("/usr/bin/codesign"),
        &["--verify", "--deep", "--strict", &app_text],
    )
    .map(|_| ())
    .map_err(|e| format!("the new app's signature does not hold: {e}"))
}

/// Runs `program` and returns what it printed, or what went wrong.
fn run(program: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{} could not start: {e}", program.display()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let said = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "{} failed: {}",
            program
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            said.trim()
        ))
    }
}

/// Lets a prepared version go, unused (the dry run's).
pub fn discard(staged: Staged) {
    if let Some(folder) = staged.app.parent() {
        let _ = fs::remove_dir_all(folder);
    }
}

/// Starts the helper, which puts `staged` in this app's place once YTFast
/// has closed (the window closes next) and opens it.
pub fn hand_over(staged: &Staged) -> Result<(), String> {
    let target = bundle()?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Command::new(exe)
        .arg("--finish-mac-update")
        .arg(std::process::id().to_string())
        .arg(&target)
        .arg(&staged.app)
        .arg(&staged.version)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("the helper could not start: {e}"))
}

/// `--finish-mac-update <pid> <app> <new app> <version>`: the helper's
/// work. Waits for YTFast (`pid`) to close, puts the new app in the old
/// one's place (the old one back if that fails), leaves the result for
/// the next start, and opens YTFast. Its exit code says whether it worked.
pub fn finish(args: &[String]) -> i32 {
    let [pid, target, staged, version] = args else {
        return 2;
    };
    let (target, staged) = (PathBuf::from(target), PathBuf::from(staged));
    let result = wait_for_exit(pid).and_then(|()| {
        let previous = target.with_file_name(".YTFast-previous.app");
        let _ = fs::remove_dir_all(&previous);
        swap(&target, &staged, &previous)?;
        let _ = fs::remove_dir_all(&previous);
        Ok(())
    });
    let said = match &result {
        Ok(()) => format!("YTFast was updated to {version}."),
        // The Mac may keep apps from changing apps (App Management).
        Err(e) if e.contains("not permitted") => format!(
            "YTFast could not be updated to {version}: your Mac did not allow it. In System Settings, Privacy & Security, App Management, turn on YTFast, then try again."
        ),
        Err(e) => format!("YTFast could not be updated to {version}: {e}"),
    };
    if let Some(folder) = folder() {
        let _ = fs::create_dir_all(&folder);
        let _ = fs::write(folder.join("result.txt"), &said);
    }
    // The new version, or the old one again.
    let _ = Command::new("/usr/bin/open").arg(&target).status();
    i32::from(result.is_err())
}

/// Waits until the process `pid` has ended, for at most [`CLOSE_WAIT`].
fn wait_for_exit(pid: &str) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < CLOSE_WAIT {
        let running = Command::new("/bin/kill")
            .args(["-0", pid])
            .status()
            .is_ok_and(|s| s.success());
        if !running {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("YTFast did not close".into())
}

/// Moves `target` aside to `previous`, and `staged` into its place; puts
/// `target` back when the second move fails. All three on one disk, as
/// Applications and YTFast's cache are on a Mac (else it copies).
fn swap(target: &Path, staged: &Path, previous: &Path) -> Result<(), String> {
    fs::rename(target, previous).map_err(|e| format!("the old app could not be moved: {e}"))?;
    if let Err(e) = move_folder(staged, target) {
        let _ = fs::remove_dir_all(target);
        return match fs::rename(previous, target) {
            Ok(()) => Err(format!("the new app could not be moved in: {e}")),
            Err(back) => Err(format!(
                "the new app could not be moved in ({e}), nor the old one back ({back})"
            )),
        };
    }
    Ok(())
}

/// Moves a folder, copying it (with `ditto`, which keeps everything an app
/// needs) when it is on another disk.
fn move_folder(from: &Path, to: &Path) -> Result<(), String> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    run(
        Path::new("/usr/bin/ditto"),
        &[&from.to_string_lossy(), &to.to_string_lossy()],
    )?;
    let _ = fs::remove_dir_all(from);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair;

    #[test]
    fn a_signed_checksum_list_is_read_and_others_are_refused() {
        let pkcs8 =
            ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                .unwrap();
        let key = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public = hex(key.public_key().as_ref());
        let manifest = b"aa11  ytfast-v1.0.0-x86_64-pc-windows-msvc.zip\nBB22  YTFast.dmg\n";
        let signature = key.sign(manifest);
        let read = checked_manifest(manifest, signature.as_ref(), &public).unwrap();
        assert_eq!(digest_for(&read, "YTFast.dmg").as_deref(), Some("bb22"));
        assert_eq!(digest_for(&read, "YTFast.exe"), None);
        // Changed by one letter, or signed by another key: refused.
        let mut changed = manifest.to_vec();
        changed[0] = b'b';
        assert!(checked_manifest(&changed, signature.as_ref(), &public).is_err());
        let other =
            ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                .unwrap();
        let other = ring::signature::Ed25519KeyPair::from_pkcs8(other.as_ref()).unwrap();
        assert!(checked_manifest(manifest, other.sign(manifest).as_ref(), &public).is_err());
        assert!(checked_manifest(manifest, b"short", &public).is_err());
    }

    #[test]
    fn the_app_is_found_from_its_program() {
        let exe = Path::new("/Applications/YTFast.app/Contents/MacOS/ytfast");
        assert_eq!(
            bundle_of(exe).as_deref(),
            Some(Path::new("/Applications/YTFast.app"))
        );
        assert_eq!(bundle_of(Path::new("/usr/local/bin/ytfast")), None);
        assert_eq!(bundle_of(Path::new("/Users/x/target/release/ytfast")), None);
    }

    /// The new app takes the old one's place; when it cannot, the old one
    /// is put back as it was.
    #[test]
    fn a_new_app_takes_the_old_ones_place_or_the_old_one_stays() {
        let dir = tempfile::tempdir().unwrap();
        let app = |name: &str, says: &str| {
            let path = dir.path().join(name);
            fs::create_dir_all(path.join("Contents")).unwrap();
            fs::write(path.join("Contents/version"), says).unwrap();
            path
        };
        let says = |path: &Path| fs::read_to_string(path.join("Contents/version")).unwrap();
        let target = app("YTFast.app", "old");
        let staged = app("new/YTFast.app", "new");
        let previous = dir.path().join(".YTFast-previous.app");
        swap(&target, &staged, &previous).unwrap();
        assert_eq!(says(&target), "new");
        assert_eq!(says(&previous), "old");
        assert!(!staged.exists());

        // Nothing to move in: the old one is back.
        fs::remove_dir_all(&previous).unwrap();
        let missing = dir.path().join("missing/YTFast.app");
        assert!(swap(&target, &missing, &previous).is_err());
        assert_eq!(says(&target), "new");
        assert!(!previous.exists());
    }

    #[test]
    fn only_a_mac_reads_a_result() {
        if !cfg!(target_os = "macos") {
            assert_eq!(take_result(), None);
        }
    }
}
