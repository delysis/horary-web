use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub const OCR_SHA256: &str = "cd5853df311b12f2ec7fcc612f49b0a5248a5c9b87ef7780730fbdcd618d32d4";
pub const DEFAULT_OCR: &str = "/Users/george/.codex/evidence/horary-readings-20261008/worker-readable-book-20261009/frawley-ocr.md";
const CARDS: &str = include_str!("../../../src-tauri/src/horary_prompts/passages.json");

#[derive(Clone)]
pub struct Book {
    pages: BTreeMap<usize, String>,
    cards: Vec<Value>,
}
impl Book {
    /// Mechanics-only tests need page availability, not access to the private
    /// OCR. These placeholders are visibly synthetic and cannot qualify a
    /// reading. Production still enters through the hash-checked `open` path.
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self {
            pages: (1..=400)
                .map(|printed| {
                    (
                        printed + 9,
                        format!(
                            "SYNTHETIC TEST PAGE {printed}: placeholder only, not a source quotation or horary procedure."
                        ),
                    )
                })
                .collect(),
            cards: serde_json::from_str(CARDS).expect("Current source cards must parse"),
        }
    }

    pub fn open(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)?;
        ensure!(
            crate::google::digest(&bytes) == OCR_SHA256,
            "Primary OCR bytes differ from the reviewed source"
        );
        let text = String::from_utf8(bytes)?;
        let marker = Regex::new(r"(?m)^## Page (\d+)\s*$")?;
        let matches = marker.captures_iter(&text).collect::<Vec<_>>();
        let mut pages = BTreeMap::new();
        for (i, page) in matches.iter().enumerate() {
            let m = page.get(0).context("No page heading")?;
            let end = matches
                .get(i + 1)
                .and_then(|next| next.get(0))
                .map_or(text.len(), |m| m.start());
            pages.insert(page[1].parse()?, text[m.end()..end].trim().to_owned());
        }
        Ok(Self {
            pages,
            cards: serde_json::from_str(CARDS)?,
        })
    }
    pub fn pages(&self, printed: &BTreeSet<usize>) -> Result<String> {
        let mut text = String::new();
        for page in printed {
            let ocr = page + 9;
            let content = self
                .pages
                .get(&ocr)
                .with_context(|| format!("Missing printed page {page}, OCR page {ocr}"))?;
            text.push_str(&format!(
                "\n<Frawley printed_page=\"{page}\" ocr_page=\"{ocr}\">\n{content}\n</Frawley>\n"
            ));
        }
        Ok(text)
    }
    pub fn context(
        &self,
        method_pages: impl IntoIterator<Item = usize>,
        reading: bool,
    ) -> Result<String> {
        self.pages(&Self::context_pages(method_pages, reading))
    }
    /// Preserve an exact page-opening continuation without importing a later,
    /// unrelated section as part of this method's source allocation.
    pub fn first_paragraph(&self, printed: usize) -> Result<String> {
        let ocr = printed + 9;
        let content = self
            .pages
            .get(&ocr)
            .with_context(|| format!("Missing printed page {printed}, OCR page {ocr}"))?;
        let paragraph = content
            .lines()
            .take_while(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        ensure!(
            !paragraph.is_empty(),
            "Missing first paragraph on printed page {printed}"
        );
        Ok(format!(
            "\n<Frawley printed_page=\"{printed}\" ocr_page=\"{ocr}\" excerpt=\"first_paragraph\">\n{paragraph}\n</Frawley>\n"
        ))
    }
    /// The exact existing full-context allocation, available to source-bound
    /// authoring receipts without parsing page markers or changing students.
    pub fn context_pages(
        method_pages: impl IntoIterator<Item = usize>,
        reading: bool,
    ) -> BTreeSet<usize> {
        let mut printed = method_pages.into_iter().collect::<BTreeSet<_>>();
        printed.extend([7, 8]);
        // Full primary pages, including the ordinary adverse case and its
        // contextual exception. A short exception card is not the entire rule.
        if reading {
            printed.extend([
                30, 40, 44, 56, 57, 58, 59, 60, 61, 62, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75,
                76, 77, 126, 127, 128, 129, 130, 131,
            ]);
        }
        printed
    }
    /// Compact exact primary witnesses; the complete chapter context is kept
    /// for the teacher/judge and available to callers, never synthesized as fact.
    pub fn primary_cards(&self) -> String {
        self.cards
            .iter()
            .map(|c| {
                format!(
                    "\n<rule id=\"{}\" printed=\"{}–{}\" ocr=\"{}–{}\">\n{}\n</rule>\n",
                    c["id"].as_str().unwrap_or_default(),
                    c["printedFirst"],
                    c["printedLast"],
                    c["ocrFirst"],
                    c["ocrLast"],
                    c["quote"].as_str().unwrap_or_default()
                )
            })
            .collect()
    }
    pub fn student_context(
        &self,
        method_pages: impl IntoIterator<Item = usize>,
        reading: bool,
    ) -> Result<String> {
        let mut pages = method_pages.into_iter().collect::<BTreeSet<_>>();
        pages.extend([7, 8]);
        let mut text = self.pages(&pages)?;
        if reading {
            let ids = [
                "simplicity",
                "no_forced_certainty",
                "significator_definition",
                "occasion_motive_ability",
                "essential_quality",
                "house_capacity",
                "retrograde_context",
                "solar_opposition_context",
                "combustion_sign",
                "solar_exceptions",
                "cazimi",
                "own_or_others_dignities",
                "no_automatic_damage",
                "reception_example",
                "reception_by_sign",
                "reception_exaltation",
                "reception_triplicity",
                "translation",
                "collection",
                "next_contacts",
                "timing_basis",
                "timing_distance",
                "timing_units",
                "timing_applicant",
                "volition",
                "timing_examples",
            ];
            for card in &self.cards {
                if ids.contains(&card["id"].as_str().unwrap_or_default())
                    && !pages.contains(&(card["printedFirst"].as_u64().unwrap_or(0) as usize))
                {
                    text.push_str(&format!(
                        "\n<rule id=\"{}\" printed=\"{}–{}\" ocr=\"{}–{}\">\n{}\n</rule>\n",
                        card["id"].as_str().unwrap_or_default(),
                        card["printedFirst"],
                        card["printedLast"],
                        card["ocrFirst"],
                        card["ocrLast"],
                        card["quote"].as_str().unwrap_or_default()
                    ));
                }
            }
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_paragraph_retains_exact_continuation_without_following_section() {
        let mut book = Book::fixture();
        book.pages.insert(
            197,
            "Exact continuation.\nSecond line.\n  \nUNRELATED TREATMENT SECTION".into(),
        );
        let source = book.first_paragraph(188).unwrap();
        assert!(
            source.contains("printed_page=\"188\" ocr_page=\"197\" excerpt=\"first_paragraph\"")
        );
        assert!(source.contains("Exact continuation.\nSecond line."));
        assert!(!source.contains("UNRELATED"));
    }
    #[test]
    fn changed_source_is_not_silently_used() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        fs::write(temp.path(), "different book").unwrap();
        assert!(
            Book::open(temp.path())
                .err()
                .unwrap()
                .to_string()
                .contains("differ")
        );
    }
}
