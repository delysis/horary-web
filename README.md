# Horary

A local-first horary chart calculator and experimental judgement workspace, based on [Eileen’s original app](https://github.com/Winterdust408/horary-web). This fork keeps her chart wheel, day/night themes, location workflow, and display controls while making the interpretation method open to review.

The desktop app uses **Rust and Tauri**, with the existing React interface. It runs without Python, Node.js, a terminal, or a hosted AI account. Development uses Rust, Node.js, and the platform’s native build tools; no Python environment is required.

## For Eileen

Start with [the review guide](docs/REVIEW_GUIDE.md). Enter a question, choose the astrologer’s location, and cast. **The method & evidence** exposes the book-based instructions and calculated testimony. Export a reading to accompany your own notes, or use the optional local review notebook.

AI is optional. Choose **Set up a local reading**, then select **Set up local model**. Horary downloads the pinned Gemma 4 12B QAT model and matching multimodal projector directly from Hugging Face, verifies them, and uses the shared Hugging Face cache without a second copy. No CLI, account, or existing weights are required. Setup supports pause/resume; readings work offline afterward. The calculator and method inspector also work without a model.

Review builds are attached to successful [CI runs](https://github.com/delysis/horary-web/actions/workflows/ci.yml). Choose the run for the review branch and download its desktop artifact. These are unsigned development builds, not a signed public release. The macOS artifact contains `Horary.app`; Windows contains `horary.exe`.

## Develop

Install a current stable Rust toolchain, Node.js 22+, and [Tauri’s native prerequisites](https://v2.tauri.app/start/prerequisites/). On macOS, Xcode Command Line Tools and CMake are needed for the native model backend.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.13.1 --locked
npm ci
npm run tauri -- dev --features native-llama-metal  # Apple Silicon / Metal
# Or: npm run tauri -- dev                       # native CPU backend
```

`npm run dev` runs the browser calculator. Rust supplies the shared prompt, validation, calendar/timezone, and review-record code through WebAssembly. The frontend build generates the necessary bindings automatically.

```sh
npm run check:all
npm run check:release-assets
npm run check:native-llama       # needs a local compatible GGUF
npm run check:horary-readings    # needs the same local model
```

Set `HORARY_NATIVE_LLAMA_TEST_MODEL` to the full model path if automatic discovery does not find it. Native-kit is pinned to an immutable revision; see [model and runtime provenance](docs/MODEL_PROVENANCE.md). Speculative decoding and image/audio attachments are not implemented in this reading interface.

Build the native macOS review app with:

```sh
npm run tauri -- build --debug --features native-llama-metal --bundles app
```

The app appears under `src-tauri/target/debug/bundle/macos/Horary.app`. macOS 11+ is required. Native inference is enabled by default; no separate inference server is needed.

## Boundaries to review

- The same calculated chart supplies the wheel, tables, and AI facts. UTC resolution uses the selected IANA timezone, with explicit handling of ambiguous or nonexistent civil times.
- The astronomical engine is an approximation checked against the committed JPL and Swiss Ephemeris fixtures. Those tests cover specific dates and tolerances, not every possible chart or professional ephemeris precision.
- House suggestions and the traditional interpretation scaffold are provisional. They have not received Eileen’s subject-matter sign-off.
- The current interpretation is one model call with a compact report of decisive steps. A reported “adversarial check” is part of that same call, not an independent verifier.
- Structured model output is validated. This does not establish the truth of its prose or the correctness of its astrological judgment.

See [architecture](docs/ARCHITECTURE.md), [privacy](docs/PRIVACY.md), [AI policy](docs/AI_POLICY.md), and [verification](docs/VERIFICATION.md).
