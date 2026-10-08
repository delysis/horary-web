fn main() {
    fn git(args: &[&str]) -> Option<String> {
        let output = std::process::Command::new("git").args(args).output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
    if let Some(path) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={path}");
    }
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &branch]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let revision = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unversioned".into());
    let dirty =
        git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    println!(
        "cargo:rustc-env=HORARY_BUILD_GIT_SHA={revision}{}",
        if dirty { "-dirty" } else { "" }
    );
    #[cfg(target_os = "macos")]
    {
        // Dependency link arguments do not propagate to application/test
        // executables. Resolve Apple's Swift runtime from the OS, not Xcode.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        println!("cargo:rerun-if-changed=native/location_bridge.m");
        println!("cargo:rerun-if-changed=native/permissions_bridge.m");
        cc::Build::new()
            .file("native/location_bridge.m")
            .file("native/permissions_bridge.m")
            .flag("-fobjc-arc")
            .compile("horary_location_bridge");
        println!("cargo:rustc-link-lib=framework=CoreLocation");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
    }

    tauri_build::build()
}
