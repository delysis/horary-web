# Horary

A private conversation with a horary reader, in one document that unfolds as you talk. Based on [Eileen’s original app](https://github.com/Winterdust408/horary-web) and John Frawley’s *The Horary Textbook*.

Speak or type. The agent clarifies the question, resolves the place and moment, calls the Rust chart tools, and develops a provisional reading on a scroll. No chart forms or settings button. Ask why, offer a correction, or ask for the supporting calculation.

The native app uses Rust, Tauri and native-kit with Gemma 4 12B IT QAT. First use prepares the model automatically in the shared Hugging Face cache, without Python, a CLI, an account, or duplicate weights. Audio is processed locally through the matching projector; macOS can speak replies using an installed system voice.

For Eileen, start with [the review guide](docs/REVIEW_GUIDE.md). [Conversation architecture](docs/CONVERSATION.md) explains the authority boundaries and current limitations. This is an unsigned development build, awaiting real conversational and domain acceptance. Review artifacts are attached to successful [CI runs](https://github.com/delysis/horary-web/actions/workflows/ci.yml).

## Develop

Install a current stable Rust toolchain, Node.js 22+, and [Tauri’s native prerequisites](https://v2.tauri.app/start/prerequisites/). On macOS, Xcode Command Line Tools and CMake are needed for the native model backend.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.13.1 --locked
npm ci
npm run tauri -- dev --features native-llama-metal  # Apple Silicon / Metal
# Or: npm run tauri -- dev                       # native CPU backend
```

`npm run dev` runs the visual preview. The actual conversation, audio and chart tools run in the native app. The retained shared-core WebAssembly build supports legacy calculator tests. Linux builds additionally require ALSA development headers for native microphone capture.

```sh
npm run check:all
npm run check:release-assets
npm run check:native-llama       # needs a local compatible GGUF
npm run check:horary-readings    # needs the same local model
```

Set `HORARY_NATIVE_LLAMA_TEST_MODEL` to the full model path if automatic discovery does not find it. Native-kit is pinned to an immutable revision; see [model and runtime provenance](docs/MODEL_PROVENANCE.md). Speculative decoding is not exposed by the pinned runtime. Voice uses native microphone WAV input and the matching projector; image attachments have no interface yet.

Build the native macOS review app with:

```sh
npm run tauri -- build --debug --features native-llama-metal --bundles app
```

The app appears under `src-tauri/target/debug/bundle/macos/Horary.app`. macOS 11+ is required. Native inference is enabled by default; no separate inference server is needed.

## Boundaries to review

- The same calculated chart supplies the wheel, tables, and AI facts. UTC resolution uses the selected IANA timezone, with explicit handling of ambiguous or nonexistent civil times.
- The astronomical engine is an approximation checked against the committed JPL and Swiss Ephemeris fixtures. Those tests cover specific dates and tolerances, not every possible chart or professional ephemeris precision.
- House suggestions and the traditional interpretation scaffold are provisional. They have not received Eileen’s subject-matter sign-off.
- The conversation uses a bounded Rust tool loop and an evolving scroll. Tool receipts and evidence pointers make it auditable; they are not an independent verifier of its prose.
- Structured model output is validated. This does not establish the truth of its prose or the correctness of its astrological judgment.

See [architecture](docs/ARCHITECTURE.md), [privacy](docs/PRIVACY.md), [AI policy](docs/AI_POLICY.md), and [verification](docs/VERIFICATION.md).
