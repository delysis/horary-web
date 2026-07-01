//! Build script for the llama-cpp-2 wrapper crate.

fn main() {
    if let Ok(dir) = std::env::var("DEP_LLAMA_BACKENDS_DIR") {
        println!("cargo:rustc-env=GGML_BACKENDS_DIR={}", dir);
    }
}
