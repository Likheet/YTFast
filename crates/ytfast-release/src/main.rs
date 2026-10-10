//! Makes YTFast's release files, on the owner's laptop, for the app's own
//! updater (fastframe-update, reading the GitHub release):
//!
//! ```text
//! ytfast-release key <key file>         make the signing key (once; keep it safe)
//! ytfast-release public <key file>      print its public half, for the app
//! ytfast-release pack <version> <key file> <out folder> <ytfast.exe> [<YTFast.dmg>]
//! ```
//!
//! `pack` writes, in the out folder:
//! - `YTFast.exe`, the Windows program, and `YTFast.dmg`, the Mac disk
//!   image (CI's, when given): what people download;
//! - `ytfast-v<version>-x86_64-pc-windows-msvc.zip`: a folder of that name
//!   holding `ytfast.exe` and the `ytfast-portable.txt` marker, the
//!   updater's portable layout, which every installed Windows copy looks
//!   for (the Mac updates by hand for now). It carries no clock time, so
//!   the same program always packs to the same bytes;
//! - `checksums.txt`, every file's SHA-256 in `sha256sum` form;
//! - `checksums.txt.sig`, the raw 64-byte Ed25519 signature of exactly
//!   `checksums.txt`'s bytes, which the app checks against the public key it
//!   was built with before it downloads anything.
//!
//! The key file holds the private key: it never goes in the repository, a
//! release or a message. Lost, no update can be signed again for the
//! copies already out there.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use ring::rand::SystemRandom;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use sha2::{Digest, Sha256};

/// The updater's name for the app (`UpdateConfig::slug`).
const SLUG: &str = "ytfast";
/// The Windows build's target, in release file names.
const WINDOWS_TARGET: &str = "x86_64-pc-windows-msvc";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["key", path] => {
            let public = make_key(Path::new(path))?;
            println!("Made the signing key in {path}. Keep a copy somewhere safe.");
            println!("Its public half (for crates/ytfast/assets/update-public-key.hex):");
            println!("{public}");
        }
        ["public", path] => println!("{}", public_key(&read_key(Path::new(path))?)),
        ["pack", version, key, out, windows, rest @ ..] => {
            let mac = rest.first().map(PathBuf::from);
            let files = pack(
                version,
                &read_key(Path::new(key))?,
                Path::new(out),
                Path::new(windows),
                mac.as_deref(),
            )?;
            for file in files {
                println!("{}", file.display());
            }
        }
        _ => bail!(
            "usage:\n  ytfast-release key <key file>\n  ytfast-release public <key file>\n  \
             ytfast-release pack <version> <key file> <out folder> <ytfast.exe> [<YTFast.dmg>]"
        ),
    }
    Ok(())
}

/// Makes a new signing key at `path` (never over an existing one) and
/// returns its public half in hexadecimal.
fn make_key(path: &Path) -> Result<String> {
    ensure!(
        !path.exists(),
        "{} already exists: a new key would stop the copies already out there from updating",
        path.display()
    );
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
        .map_err(|_| anyhow::anyhow!("could not make a key"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, hex(pkcs8.as_ref()))?;
    Ok(public_key(&read_key(path)?))
}

fn read_key(path: &Path) -> Result<Ed25519KeyPair> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let bytes = unhex(text.trim()).context("the key file is not hexadecimal")?;
    Ed25519KeyPair::from_pkcs8(&bytes).map_err(|_| anyhow::anyhow!("the key file holds no key"))
}

fn public_key(key: &Ed25519KeyPair) -> String {
    hex(key.public_key().as_ref())
}

/// Writes the release files for `version` into `out` and returns them.
fn pack(
    version: &str,
    key: &Ed25519KeyPair,
    out: &Path,
    windows: &Path,
    mac: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    ensure!(
        !version.is_empty()
            && version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())),
        "the version is three numbers, as 0.6.0"
    );
    fs::create_dir_all(out)?;
    let mut files = Vec::new();

    let stem = format!("{SLUG}-v{version}-{WINDOWS_TARGET}");
    let archive = out.join(format!("{stem}.zip"));
    let program = fs::read(windows).with_context(|| format!("reading {}", windows.display()))?;
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(format!("{stem}/{SLUG}.exe"), options)?;
    zip.write_all(&program)?;
    zip.start_file(format!("{stem}/{SLUG}-portable.txt"), options)?;
    zip.write_all(format!("{SLUG}-portable-v1\n").as_bytes())?;
    zip.finish()?;
    files.push(archive);

    let download = out.join("YTFast.exe");
    fs::write(&download, &program)?;
    files.push(download);

    if let Some(mac) = mac {
        ensure!(
            mac.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dmg")),
            "the Mac build is CI's disk image, YTFast.dmg"
        );
        let copy = out.join("YTFast.dmg");
        fs::copy(mac, &copy).with_context(|| format!("copying {}", mac.display()))?;
        files.push(copy);
    }

    let mut checksums = String::new();
    for file in &files {
        let digest = Sha256::digest(fs::read(file)?);
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        checksums.push_str(&format!("{}  {name}\n", hex(&digest)));
    }
    let list = out.join("checksums.txt");
    fs::write(&list, &checksums)?;
    let signature = key.sign(checksums.as_bytes());
    // Checked as the app will check it, before anything is published.
    UnparsedPublicKey::new(&ED25519, key.public_key().as_ref())
        .verify(checksums.as_bytes(), signature.as_ref())
        .map_err(|_| anyhow::anyhow!("the signature does not verify"))?;
    let signed = out.join("checksums.txt.sig");
    fs::write(&signed, signature.as_ref())?;
    files.push(list);
    files.push(signed);
    Ok(files)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_is_packed_signed_and_checkable() {
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join("keys/signing.hex");
        let public = make_key(&key_file).unwrap();
        // Never over an existing key.
        assert!(make_key(&key_file).is_err());
        let key = read_key(&key_file).unwrap();
        assert_eq!(public, public_key(&key));
        assert_eq!(public.len(), 64);

        let exe = dir.path().join("YTFast.exe");
        fs::write(&exe, b"not really a program").unwrap();
        let mac = dir.path().join("mac.dmg");
        fs::write(&mac, b"not really a Mac app").unwrap();
        let out = dir.path().join("out");
        let files = pack("0.6.0", &key, &out, &exe, Some(&mac)).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "ytfast-v0.6.0-x86_64-pc-windows-msvc.zip",
                "YTFast.exe",
                "YTFast.dmg",
                "checksums.txt",
                "checksums.txt.sig",
            ]
        );
        assert_eq!(fs::read(&files[1]).unwrap(), b"not really a program");

        // The archive holds the program and the marker in the updater's
        // folder.
        let archive = fs::File::open(&files[0]).unwrap();
        let mut zip = zip::ZipArchive::new(archive).unwrap();
        let mut inside: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        inside.sort();
        assert_eq!(
            inside,
            [
                "ytfast-v0.6.0-x86_64-pc-windows-msvc/ytfast-portable.txt",
                "ytfast-v0.6.0-x86_64-pc-windows-msvc/ytfast.exe",
            ]
        );

        // checksums.txt names each file once, in sha256sum form, and its
        // signature verifies with the public key alone.
        let checksums = fs::read_to_string(&files[3]).unwrap();
        let lines: Vec<&str> = checksums.lines().collect();
        assert_eq!(lines.len(), 3);
        let digest = hex(&Sha256::digest(fs::read(&files[0]).unwrap()));
        assert_eq!(
            lines[0],
            format!("{digest}  ytfast-v0.6.0-x86_64-pc-windows-msvc.zip")
        );
        let signature = fs::read(&files[4]).unwrap();
        assert_eq!(signature.len(), 64);
        let public = unhex(&public).unwrap();
        assert!(
            UnparsedPublicKey::new(&ED25519, &public)
                .verify(checksums.as_bytes(), &signature)
                .is_ok()
        );
        assert!(
            UnparsedPublicKey::new(&ED25519, &public)
                .verify(b"something else", &signature)
                .is_err()
        );
        // Packed again, the update archive is the same to the byte, so
        // files can be added to a release without changing it.
        let archive = fs::read(&files[0]).unwrap();
        let again = dir.path().join("again");
        let repacked = pack("0.6.0", &key, &again, &exe, None).unwrap();
        assert_eq!(fs::read(&repacked[0]).unwrap(), archive);

        assert!(pack("0.6", &key, &out, &exe, None).is_ok());
        assert!(pack("v0.6.0", &key, &out, &exe, None).is_err());
        // The Mac build is a disk image, not a zip.
        assert!(pack("0.6.0", &key, &out, &exe, Some(&dir.path().join("mac.zip"))).is_err());
    }
}
