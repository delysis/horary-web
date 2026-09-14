# Privacy and network behavior

The conversation, charts, place lookup, microphone audio and interpretation run locally. There is no analytics, hosted inference, remote dictation, remote font, feedback upload or automatic synchronization.

The first message prepares the pinned Gemma 4 12B IT QAT model and matching projector. Missing files are downloaded through native Rust HTTPS from Hugging Face and its delivery infrastructure. Hugging Face receives ordinary download requests and network metadata, never the question, chart or recording. The chosen artifacts are public and require no account or token. Setup respects the shared Hugging Face cache and verifies complete SHA-256 digests. Stop can pause an acquisition for later resumption.

The cache location follows HF_HUB_CACHE, HUGGINGFACE_HUB_CACHE, HF_HOME, XDG_CACHE_HOME, then ~/.cache/huggingface/hub. Horary shares blob locks and resumable .incomplete files, publishes verified blobs and snapshot links, and leaves refs/main untouched. Removing cached weights requires setup again. No speculative helper is currently used by the pinned native-kit API.

Microphone capture begins only from the microphone control and ends when the user finishes/cancels, the UI's two-minute timer expires, or the app exits. The native recorder additionally bounds memory. Audio is held in memory for local Gemma transcription, then dropped; it is not saved in the conversation. The visible transcript is saved. macOS spoken replies use an installed system voice; text is passed as a process argument to the local speech utility, without a shell. No cloud voice is selected.

Conversation text, question, resolved place, chart, scroll revisions and tool receipts are atomically saved as `conversation.json` in native app data. These may contain sensitive personal details. The application does not upload or encrypt this file. Legacy charts and review notes remain in their original local stores and are not deleted by the redesign.

The browser URL is a visual preview. It does not record through browser speech recognition or send a question to an AI server. The native app is required for the conversation. Legacy browser inference/export modules remain in the repository but are not entry points in the new interface.
