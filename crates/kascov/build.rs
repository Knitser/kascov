//! Build provenance: stamps KASCOV_GIT_HASH into the binary so /healthz can
//! answer `build` and the deploy script can assert the running container is
//! the commit it just shipped. A pre-set KASCOV_GIT_HASH in the environment
//! wins — the Docker build context carries crates/** only, no .git, so the
//! deploy script exports the hash into the build instead. With neither
//! source the value is "unknown", never a guess.

fn main() {
    println!("cargo:rerun-if-env-changed=KASCOV_GIT_HASH");
    let preset = std::env::var("KASCOV_GIT_HASH")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let hash = preset.unwrap_or_else(git_short_head);
    println!("cargo:rustc-env=KASCOV_GIT_HASH={hash}");
}

/// `git rev-parse --short HEAD` at the workspace root, or "unknown" where
/// there is no repository to ask. Watches HEAD (moves on checkout) and the
/// ref it names (moves on commit) so a new commit re-stamps the binary;
/// only paths that exist are declared, so a .git-less build does not re-run
/// this script on every compile.
fn git_short_head() -> String {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let repo = std::path::Path::new(&manifest).join("../..");
    let head = repo.join(".git/HEAD");
    if head.exists() {
        println!("cargo:rerun-if-changed={}", head.display());
        if let Ok(content) = std::fs::read_to_string(&head) {
            if let Some(r) = content.strip_prefix("ref: ") {
                let ref_path = repo.join(".git").join(r.trim());
                if ref_path.exists() {
                    println!("cargo:rerun-if-changed={}", ref_path.display());
                }
            }
        }
    }
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(&repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}
