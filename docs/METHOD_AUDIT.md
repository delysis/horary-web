# Book-method audit

Working implementation checked against John Frawley, *The Horary Textbook* (2005), private local OCR. Page numbers below are printed pages; main-text PDF pages in the reviewed scan are nine greater. This record is an engineering/source audit, not Eileen's subject-matter approval.

| Topic | Printed pages | Implementation and review boundary |
| --- | --- | --- |
| Question, moment and place | 7–14, 137–141 | Visible cast context; civil time/IANA resolution in `chart_input.rs`. Meaning of the question remains the astrologer's decision. |
| Houses and significators | 15–38 | Traditional cusp rulers, provisional question hints, small animals house 6 and large animals house 12. Turning/ownership remains contextual. |
| Near-cusp placement | 17, 56 | Approximately five degrees before the next cusp, in that cusp's sign. `geometricHouse` preserves the astronomical placement; adjusted judgment house is inspectable. |
| Essential dignity | 44–53, 71–75 | `book_method.rs` implements the printed p. 72 table, including water triplicity Mars by day and night and exclusive term boundaries. No summed strength score. |
| Peregrine | 50–51 | No own dignity, detriment, or fall under this author's usage. Missing sect remains unknown where needed. |
| Accidental ability | 54–70 | Angular houses strong; 6, 8, 12 weak; other houses neutral for capacity. Cadency remains distinct for timing. Context may alter meaning. |
| Combustion and visibility | 61–63 | Cazimi within 17.5 arcminutes; combustion within 8.5 degrees and same sign; under beams within 17.5 degrees. Contextual exceptions are not automatic overrides. |
| Reception | 71–83 | Guest toward host, separate dignity/debility entries, including detriment and fall. No assumption that mutual reception is positive. |
| Void Moon and long gaps | 65–66 | Strict no-contact-before-exit status plus a separate review flag for a gap of roughly 15 degrees or more. The latter is contextual, not an automatic veto. |
| Aspects and occasions | 84–100 | Display aspects retain their orb policy. Future candidates use all major crossings across hourly samples without a starting-orb filter. Sign changes are recorded. |
| Timing | 127–136 | Interpolated astronomical hours are identified as estimates; they are not converted mechanically to symbolic dates. |
| Lost objects | 146–155 | Distinguish location from recovery; a void Moon is not a veto on identifying a location. |

## Regression evidence

Rust tests cover table boundaries, sect, solar sign boundaries and longitude wrap, qualitative capacity, directed negative reception, cusp advancement, and repeated normalization. Event tests cover a contact in the final fraction of the Moon's sign, a contact after exit, contacts beyond a display orb, and unknown status for incomplete coverage.

The existing astronomical engine uses approximate positions. Book fidelity in the method layer cannot compensate for an inaccurate longitude or cusp. Compare boundary cases with Eileen's trusted ephemeris. Hourly event sampling does not certify stations or reversals between samples, nor does it independently establish translation, collection, prohibition, frustration, or refranation. Fixed stars, lots and antiscia also retain explicit source/precision review needs.

The model's findings remain hypotheses. Its evidence references and method instructions are available for inspection and export. A passing output-schema test is not a correct reading.
