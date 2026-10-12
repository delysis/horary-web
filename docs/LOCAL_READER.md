# Local reading runtime

Horary acquires the pinned Gemma 4 12B IT QAT target, image/audio projector and
253 MB MTP helper through its existing verified shared Hugging Face cache. The
helper is paired with that target and is never offered as a primary model.

The conversation and the older interpretation command use one selected owner:

- An explicit HORARY_LOCAL_LLAMA_ENDPOINT=http://127.0.0.1:63924 attaches to an
  already running loopback server. Horary checks its reported model file against
  the selected cached file, starts no second model and never kills that owner.
  A missing, wrong or unreachable explicit endpoint stops preparation.
- When a compatible installed/bundled sidecar and the paired helper are
  available, the existing process owner starts them together with the matching
  projector. Draft devices follow the target; helper and target share the
  appropriate KV storage. The conversation reserves 32k per slot, rather than
  splitting one 32k arena into four insufficient 8k slots. Application shutdown
  joins only its own child. A reused owner with insufficient per-slot context
  stops preparation before any reading is sent.
- Otherwise the existing native-kit path remains available. Its current pinned
  runtime still does not support MTP. Downloading a helper does not qualify that
  runtime as speculative.

Initial attachment verifies the selected cached payload; subsequent turns reuse
that resident registration while rechecking the server's reported file and
context. This avoids rehashing the full weights on each conversational turn.
Borrowed server metadata establishes that route and file identity, not an
independent attestation of its drafter or every loaded tensor.

The compatible Gemma server is a separately installed/bundled executable.
This change does not supply or sign a new server bundle. A server launched
without the projector can answer text; direct audio checks its actual
/props audio capability and stops before sending audio when absent. It does
not secretly transcribe or start another model. Native dictation remains
available through the existing voice route.

Each stage keeps its original message array, repairs, native evidence checks and
output limit. Independent text tasks use the existing batch, up to four HTTP
requests. A failure or uncertain transport is retained rather than automatically
resubmitted. Cancellation drops that request. Ordinary callers omit decoder
constraints; an existing explicitly constrained caller retains its requested
schema. Cache and token metrics are taken from the observed server response.

Offline routing, request/response and ownership checks establish the wiring.
Actual MTP throughput, multimodal correctness, horary interpretation and signed
application acceptance require separate measured journeys.
