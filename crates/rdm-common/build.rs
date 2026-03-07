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

    // Read auto-incrementing build number from .build_number
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let build_number_path = std::path::Path::new(&manifest_dir)
        .parent() // crates/
        .and_then(|p| p.parent()) // repo root
        .map(|p| p.join(".build_number"))
        .unwrap_or_else(|| std::path::PathBuf::from(".build_number"));
    let build_number = std::fs::read_to_string(&build_number_path)
        .unwrap_or_else(|_| "0".into())
        .trim()
        .to_string();

    println!("cargo:rustc-env=RDM_BUILD_ID={}", build_id);
    println!("cargo:rustc-env=RDM_BUILD_NUMBER={}", build_number);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed={}", build_number_path.display());
}
