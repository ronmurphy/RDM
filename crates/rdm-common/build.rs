use std::process::Command;

fn main() {
    // Generate build ID from git short hash + timestamp
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into());

    let timestamp = chrono::Local::now().format("%Y%m%d.%H%M").to_string();
    let build_id = format!("{}.{}", timestamp, git_hash.trim());

    println!("cargo:rustc-env=RDM_BUILD_ID={}", build_id);
    println!("cargo:rerun-if-changed=.git/HEAD");
}
