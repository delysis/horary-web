# Apple Silicon local reader bundle

The macOS MTP overlay places the compatible server and its eight libraries in
Horary.app/Contents/Resources/binaries. The existing runtime already searches
this directory and selects the paired cached Gemma helper automatically. The
GGUF weights and projector remain in the shared verified Hugging Face cache;
they are not copied into the application.

Prepare the server built from llama.cpp revision
8212c7802455255460ab8e18fc34754560031b34 in src-tauri/binaries, using the exact
names in tauri.mtp-macos.conf.json. Set CMAKE_OSX_DEPLOYMENT_TARGET=13.0,
GGML_NATIVE=OFF and GGML_BLAS=OFF in a separate build directory, retaining
Metal, the embedded shader sources and CPU Accelerate. The optional Apple BLAS
backend calls an unguarded API introduced in macOS 13.3; it is not included in
this 13.0 profile. Disabling host CPU specialization also avoids packaging this
Mac's instruction-set extensions as an Apple Silicon baseline.

In packaging copies only, replace the absolute build-directory LC_RPATH with
@loader_path in the server and its
libraries. Preserve the complete library group; this build embeds its Metal
shader source. Sign the prepared Mach-O files, then write the server's final
SHA-256 to llama-server-aarch64-apple-darwin.sha256. The original running server
and its libraries must remain unchanged.

The overlay retains the application's macOS 13.0 minimum. Check the deployment
version of every prepared Mach-O file; the development build's implicit minimum
must not determine the release minimum. A successful cross-target build and
version check do not qualify a reading on an older macOS machine.

Build with npm run tauri:build:macos-mtp. Its existing release-asset gate checks
the prepared server checksum, executable mode and unchanged model/runtime
pins. The Tauri overlay requires every listed resource and retains the audio
and upstream server license notices. Seal and verify the resulting application
with the existing codesign procedure. This is an Apple Silicon profile; it does
not claim matching server artifacts for the other release architectures.

Normal application launch needs no reader endpoint environment variable.
With no independent reader running, an owned-launch journey must
confirm one child, the paired helper/projector, direct audio and a completed
reading. Resource placement and signatures alone do not establish those results.
