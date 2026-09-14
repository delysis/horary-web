# Reviewing Horary

This version is a working application of John Frawley's *The Horary Textbook* (2005), intended for Eileen's assessment and correction. The model's reading is a proposal; Eileen remains the astrology authority.

## Cast a chart

Enter the question and choose the astrologer's location from the offline city list. Use the place and moment at which the astrologer understands the question. **Chart settings → Set a particular moment** lets you enter a historical chart. Check the timezone; repeated civil times require a first/second occurrence, and nonexistent times are rejected.

**Cast chart** records the current moment for the calculator. **Explore this moment** moves the chart by minutes, hours, days, weeks, months or years; **Original moment** returns to the cast. The wheel and tables use the same underlying calculation as the reading. Display aspects and upcoming astronomical contacts are separate: display orbs do not limit the event search.

## Read locally

Choose **Set up a local reading**, then **Set up local model**. The desktop app downloads Google's Gemma 4 12B IT QAT and its matching projector into the shared Hugging Face cache. No account, terminal, CLI, Python environment, or pre-existing weights are needed. Setup uses up to 7.15 GB of storage; 24 GB or more memory is recommended. Lower-memory machines may not fit the model and context.

**Pause setup** retains downloaded bytes, and **Resume model setup** continues. Already cached verified files work without a network connection. Once installed, **Read chart** produces an offline reading. You can cancel it; changing its question or chart clears the previous result. Advanced local GGUF import remains available in the reading setup details.

The reading interface currently accepts text and calculated chart facts. It does not yet accept image/audio attachments or use speculative decoding. A browser build uses a separate, optional local LiteRT model path; the packaged desktop application is the review target.

## Inspect and take notes

Open **The method & evidence** below the chart. Each step shows its purpose, printed book references, instruction, and any finding reported by the model. **Calculated chart evidence** exposes named dignities, directed receptions, adjusted house placements, and upcoming contact estimates. The model reports selected steps from one call; it is not an independent verifier.

Use whatever note-taking method suits you. **Export this reading for review** preserves the question, chart, model response, method and app version in a local JSON file that can accompany your notes. **My review notes** is an optional built-in notebook; it captures the chart and reading when you begin a note. Nothing is sent automatically.

Useful first comparisons are a familiar chart, a turned-house question, a case where dignity and reception differ, a near-cusp planet, a void or long-gap Moon, and a case involving translation or prevention. Note unclear language and missing controls as well as astrology corrections.

Review builds are unsigned development artifacts. They are not yet signed/notarized public releases. See [verification](VERIFICATION.md) for the actual checks and unresolved boundaries of this build.
