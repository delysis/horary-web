fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=native/location_bridge.m");
        cc::Build::new()
            .file("native/location_bridge.m")
            .flag("-fobjc-arc")
            .compile("horary_location_bridge");
        println!("cargo:rustc-link-lib=framework=CoreLocation");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }

    tauri_build::build()
}
