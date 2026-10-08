//! Puts YtFast's icon and name into the Windows program file, and notes
//! which code a build is (for the log and Settings).

use std::process::Command;

fn main() {
    let resource = "../../packaging/windows/ytfast.rc";
    println!("cargo::rerun-if-changed={resource}");
    println!("cargo::rerun-if-changed=../../packaging/icons/ytfast.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile(resource, embed_resource::NONE)
            .manifest_optional()
            .expect("the Windows resources compile");
    }
    println!("cargo::rustc-env=YTFAST_COMMIT={}", commit());
}

/// The commit this build is made from: CI's, else the checkout's, with "+"
/// when the checkout has changes not yet committed. "unknown" without git.
fn commit() -> String {
    println!("cargo::rerun-if-env-changed=GITHUB_SHA");
    if let Ok(sha) = std::env::var("GITHUB_SHA") {
        return sha.chars().take(7).collect();
    }
    // Look again when the checkout moves to another commit, and when the
    // code changes (for the "+").
    for path in [
        git(&["rev-parse", "--git-path", "HEAD"]),
        git(&["symbolic-ref", "-q", "HEAD"])
            .and_then(|branch| git(&["rev-parse", "--git-path", &branch])),
        git(&["rev-parse", "--git-path", "packed-refs"]),
    ]
    .into_iter()
    .flatten()
    {
        println!("cargo::rerun-if-changed={path}");
    }
    println!("cargo::rerun-if-changed=src");
    println!("cargo::rerun-if-changed=../ytfast-core/src");
    let Some(head) = git(&["rev-parse", "--short=7", "HEAD"]) else {
        return "unknown".into();
    };
    let changed = git(&["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|status| !status.is_empty());
    if changed { format!("{head}+") } else { head }
}

/// What git prints for `args`, trimmed; `None` if it could not run.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string())
}
