# Recommended model provenance

The selected model is the same Gemma 4 12B IT QAT used by Loom. Horary does not blend distributions or download a Base variant.

Both pinned artifacts come from [Google's Gemma GGUF repository](https://huggingface.co/google/gemma-4-12B-it-qat-q4_0-gguf/tree/29d097773436b69ff9feafd636ab4cf873786537). Exact SHA-256 hashes and lengths are in `src-tauri/model-manifest.json`.

| Role | File | Bytes |
| --- | --- | --- |
| Model | `gemma-4-12b-it-qat-q4_0.gguf` | 6,975,879,296 |
| Multimodal projector | `mmproj-gemma-4-12b-it-qat-q4_0.gguf` | 175,115,616 |

Revision: `29d097773436b69ff9feafd636ab4cf873786537`. Total: 7,150,994,912 bytes (7.15 decimal GB). The public repository advertises Apache-2.0; no account or automatic gated-license acceptance is needed. The app's 24 GB minimum / 32 GB recommended memory guidance is provisional, not a performance guarantee. A 16,384-token context is requested.

The matching projector is loaded with the model. The document conversation supplies text and calculated chart facts. Native microphone WAV input is transcribed by the same model through native-kit and this projector; a generated spoken-ring fixture has been transcribed successfully. Image attachments have no interface yet. The pinned native-kit controlled-generation API does not expose MTP speculative decoding, so no helper speedup is claimed. Previously cached helper weights are preserved.

Native-kit is pinned to `delysis/native-platform` revision `1d43858953938cb28e935de7c2f107eede69ddf9`, with llama-cpp-2 0.1.154 at `eb0e47b57c2fba97ed13e8fe5e949d11798232cb`. Cargo.lock records the full dependency graph. CPU and Metal builds do not invoke a Python model loader, converter or CLI.

The downloader implements the pinned-artifact subset of [Hugging Face's cache conventions](https://huggingface.co/docs/huggingface_hub/guides/manage-cache) in Rust using reqwest, SHA-256, locks, and atomic publication. It respects the configured Hub cache, reuses verified cached files offline, and stores no app-specific copy of weights. Snapshots use relative symlinks on Unix and hardlinks on Windows.

Integrity failures do not overwrite pre-existing shared blobs. New bad downloads are discarded without being registered as ready. Locks serialize downloads; interrupted downloads preserve `.incomplete` files and validate HTTP ranges before resuming. Setup verifies both files before declaring completion.
