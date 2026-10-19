use anyhow::{Context, Result, ensure};
use lopdf::{Document, LoadOptions};

use super::{
    EXTRACTOR_VERSION, Extraction, PageEvidence, PageSource, Passage, PassageTopic,
    content::{MAX_PASSAGE_CHARS, MAX_PASSAGES, MAX_TOTAL_PASSAGE_CHARS},
};

const MAX_PDF_PAGES: usize = 256;
const MAX_DECOMPRESSED_BYTES: usize = 8 * 1024 * 1024;

pub(super) fn extract(_source: &PageSource, bytes: &[u8]) -> Result<Extraction> {
    ensure!(
        bytes.len() as u64 <= super::MAX_DOCUMENT_BYTES,
        "PDF exceeds the 8 MiB document limit"
    );
    ensure!(bytes.starts_with(b"%PDF-"), "document is not a PDF");
    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            strict: true,
            max_decompressed_size: Some(MAX_DECOMPRESSED_BYTES),
            ..Default::default()
        },
    )
    .context("could not parse PDF")?;
    ensure!(
        !document.is_encrypted() && !document.was_encrypted(),
        "encrypted PDFs are unsupported"
    );
    let pages = document.get_pages();
    ensure!(
        !pages.is_empty() && pages.len() <= MAX_PDF_PAGES,
        "PDF must contain between 1 and {MAX_PDF_PAGES} pages"
    );
    let mut out = Extraction {
        extractor_version: EXTRACTOR_VERSION,
        title: None,
        published_at: None,
        canonical_url: None,
        links: vec![],
        catalog_entries: vec![],
        passages: vec![],
        claims: vec![],
        warnings: vec![],
    };
    let mut total_chars = 0;
    'pages: for number in pages.keys() {
        let text = document
            .extract_text_with_limit(&[*number], MAX_DECOMPRESSED_BYTES)
            .with_context(|| format!("could not extract PDF page {number}"))?;
        let mut remaining = text.trim();
        while !remaining.is_empty() {
            if out.passages.len() == MAX_PASSAGES || total_chars == MAX_TOTAL_PASSAGE_CHARS {
                out.warnings.push(format!(
                    "PDF selection limit reached at page {number}; later text was omitted."
                ));
                break 'pages;
            }
            let limit = MAX_PASSAGE_CHARS.min(MAX_TOTAL_PASSAGE_CHARS - total_chars);
            let end = passage_end(remaining, limit);
            let selected = remaining[..end].trim();
            total_chars += selected.chars().count();
            out.passages.push(Passage {
                topic: PassageTopic::Overview,
                heading: None,
                records: Vec::new(),
                text: selected.into(),
                evidence: PageEvidence {
                    locator: format!("pdf:page:{number}"),
                    excerpt: selected
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(240)
                        .collect(),
                },
            });
            remaining = remaining[end..].trim_start();
        }
    }
    ensure!(
        !out.passages.is_empty(),
        "PDF has no extractable text; scanned documents require OCR"
    );
    Ok(out)
}

fn passage_end(text: &str, max_chars: usize) -> usize {
    let Some((end, _)) = text.char_indices().nth(max_chars) else {
        return text.len();
    };
    // Keep a contiguous slice of the extracted page, preferably ending at a paragraph or word.
    let prefix = &text[..end];
    prefix
        .rfind("\n\n")
        .or_else(|| prefix.rfind(char::is_whitespace))
        .filter(|boundary| *boundary > 0)
        .unwrap_or(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{
        EncryptionState, EncryptionVersion, Object, Permissions, Stream,
        content::{Content, Operation},
        dictionary,
    };

    fn source() -> PageSource {
        serde_json::from_value(serde_json::json!({
            "id": "test-pdf",
            "vendor": "anthropic",
            "kind": "model_card",
            "url": "https://assets.anthropic.com/model-card.pdf",
            "format": "pdf",
            "records": ["model:test"]
        }))
        .unwrap()
    }

    fn document(texts: &[&str]) -> Document {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id }
        });
        let pages: Vec<Object> = texts
            .iter()
            .map(|text| {
                let mut operations = vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                ];
                for (index, line) in text.split('\n').enumerate() {
                    if index > 0 {
                        operations.push(Operation::new("T*", vec![]));
                    }
                    operations.push(Operation::new("Tj", vec![Object::string_literal(line)]));
                }
                operations.push(Operation::new("ET", vec![]));
                let content = Content { operations };
                let content_id =
                    document.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
                document
                    .add_object(dictionary! {
                        "Type" => "Page", "Parent" => pages_id, "Contents" => content_id
                    })
                    .into()
            })
            .collect();
        document.objects.insert(
            pages_id,
            dictionary! {
                "Type" => "Pages", "Kids" => pages, "Count" => texts.len() as i64,
                "Resources" => resources_id, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()]
            }
            .into(),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        document
    }

    fn bytes(document: &mut Document) -> Vec<u8> {
        let mut bytes = vec![];
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn preserves_page_text_and_page_evidence_without_inferred_claims() {
        let text = "First  paragraph.\nSecond paragraph.";
        let result = extract(&source(), &bytes(&mut document(&[text, "Second page."]))).unwrap();
        assert_eq!(result.passages.len(), 2);
        assert_eq!(result.passages[0].text, text);
        assert_eq!(result.passages[0].evidence.locator, "pdf:page:1");
        assert_eq!(result.passages[1].evidence.locator, "pdf:page:2");
        assert!(result.passages[0].records.is_empty());
        assert_eq!(
            result.passages[0].evidence.excerpt,
            "First paragraph. Second paragraph."
        );
        assert!(result.claims.is_empty());
    }

    #[test]
    fn bounds_selected_text_and_keeps_contiguous_original_fragments() {
        let text = "Original wording. ".repeat(500);
        let result = extract(&source(), &bytes(&mut document(&[&text]))).unwrap();
        assert!(result.passages.len() <= MAX_PASSAGES);
        assert!(
            result
                .passages
                .iter()
                .all(|p| { p.text.chars().count() <= MAX_PASSAGE_CHARS && text.contains(&p.text) })
        );
        assert!(
            result
                .passages
                .iter()
                .map(|p| p.text.chars().count())
                .sum::<usize>()
                <= MAX_TOTAL_PASSAGE_CHARS
        );
        assert_eq!(result.warnings.len(), 1);
        let unicode = "模".repeat(MAX_PASSAGE_CHARS + 1);
        assert_eq!(
            passage_end(&unicode, MAX_PASSAGE_CHARS),
            MAX_PASSAGE_CHARS * 3
        );
    }

    #[test]
    fn rejects_invalid_empty_oversize_and_excessive_page_documents() {
        assert!(extract(&source(), b"not a PDF").is_err());
        assert!(extract(&source(), b"%PDF-1.5\ninvalid").is_err());
        let mut oversize = vec![b' '; super::super::MAX_DOCUMENT_BYTES as usize + 1];
        oversize[..5].copy_from_slice(b"%PDF-");
        assert!(
            extract(&source(), &oversize)
                .unwrap_err()
                .to_string()
                .contains("8 MiB")
        );
        assert!(
            extract(&source(), &bytes(&mut document(&[""])))
                .unwrap_err()
                .to_string()
                .contains("no extractable text")
        );
        assert!(
            extract(
                &source(),
                &bytes(&mut document(&vec!["text"; MAX_PDF_PAGES + 1]))
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_encryption_even_when_the_empty_password_opens_the_document() {
        let mut document = document(&["Encrypted text"]);
        document.trailer.set(
            "ID",
            vec![
                Object::string_literal(vec![1; 16]),
                Object::string_literal(vec![2; 16]),
            ],
        );
        let state = EncryptionState::try_from(EncryptionVersion::V2 {
            document: &document,
            owner_password: "",
            user_password: "",
            key_length: 128,
            permissions: Permissions::all(),
        })
        .unwrap();
        document.encrypt(&state).unwrap();
        assert!(
            extract(&source(), &bytes(&mut document))
                .unwrap_err()
                .to_string()
                .contains("encrypted PDFs")
        );
    }
}
