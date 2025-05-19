use std::process::Command;

fn main() {
    // Git hash
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    // Build timestamp
    let build_time = chrono::Utc::now().to_rfc3339();

    // rustc version
    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=PYWATT_GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=PYWATT_BUILD_TIME_UTC={}", build_time);
    println!("cargo:rustc-env=PYWATT_RUSTC_VERSION={}", rustc_version);

    // Rebuild if git HEAD changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
}
