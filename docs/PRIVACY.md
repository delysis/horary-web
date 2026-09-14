# Privacy and network behavior

Charts, location lookup, questions, interpretations, and review notes are processed locally. The app has no analytics, remote fonts, hosted AI API, or automatic feedback upload.

The web version downloads the application assets from its host. City and US postal-code searches use bundled data. Coordinate-to-timezone lookup is offline and can be corrected by the user. Browser/OS geolocation may request permission; manual location entry remains available if permission is denied.

Selecting **Set up local model** downloads pinned public model files from Hugging Face and its delivery infrastructure using native Rust HTTPS. Hugging Face receives the download request and network metadata, never the question or chart. No account or token is required for the selected files. No download begins automatically when casting a chart. Model weights live in the shared Hugging Face file cache; the app stores only its cache registration, inference caches, and logs in app data. The advanced manual import copies a user-selected GGUF into app data. A paired assistant model accelerates generation through speculative decoding. Optional explicit sidecar builds use a loopback server; the ordinary native build performs inference in-process.

Browser AI uses a user-selected `.litertlm` file. Its runtime binaries ship as same-origin assets; there is no automatic model download or remote inference fallback. The selected file and question are not sent to a model host.

Display preferences and review notes are stored locally in the browser/webview’s storage. Each review note includes the question, UTC time, coordinates, chart facts, completed interpretation if present, method step if selected, and build identifier. The app captures this context when the note begins so later edits do not change the evidence attached to it.

**Export review notes** creates a local JSON download. The export includes potentially private questions and precise locations; the user chooses whether and where to share it. There is no automatic synchronization or backup. Clearing app/browser data can remove notes, so export before clearing data or moving devices.

The backend also contains chart-history and settings APIs inherited from earlier work. Those APIs are not represented as active history/import/export controls in the current React interface. No saved history is uploaded.

The bundled model manifest pins repositories, immutable revisions, filenames, byte lengths and SHA-256 checksums. Setup honors HF_HUB_CACHE, the legacy HUGGINGFACE_HUB_CACHE, HF_HOME, and XDG_CACHE_HOME. The default is ~/.cache/huggingface/hub. Horary shares per-blob locks and resumable .incomplete files with Hub clients, publishes verified blobs and snapshot links, and leaves refs/main untouched. Removing cached weights makes setup necessary again.
