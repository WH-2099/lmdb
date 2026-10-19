use scraper::ElementRef;

use super::{
    model::{Extraction, Passage, PassageTopic},
    page::{Page, original_text},
};

pub(super) const MAX_PASSAGES: usize = 6;
pub(super) const MAX_PASSAGE_CHARS: usize = 2_000;
pub(super) const MAX_TOTAL_PASSAGE_CHARS: usize = 6_000;

pub fn push(
    out: &mut Extraction,
    page: &Page,
    node: ElementRef<'_>,
    topic: PassageTopic,
    heading: Option<&str>,
    records: &[String],
) {
    let text = original_text(node);
    if text.is_empty() {
        return;
    }
    let evidence = page.evidence(node);
    if out.passages.iter().any(|passage| passage.text == text) {
        return;
    }
    let chars = text.chars().count();
    let total: usize = out
        .passages
        .iter()
        .map(|passage| passage.text.chars().count())
        .sum();
    let reason = if chars > MAX_PASSAGE_CHARS {
        Some(format!(
            "{chars} characters exceeds the passage limit of {MAX_PASSAGE_CHARS}"
        ))
    } else if out.passages.len() >= MAX_PASSAGES {
        Some(format!("the page already has {MAX_PASSAGES} passages"))
    } else if total + chars > MAX_TOTAL_PASSAGE_CHARS {
        Some(format!(
            "adding {chars} characters exceeds the page limit of {MAX_TOTAL_PASSAGE_CHARS}"
        ))
    } else {
        None
    };
    if let Some(reason) = reason {
        let warning = format!("Passage skipped at {}: {reason}.", evidence.locator);
        if !out.warnings.contains(&warning) {
            out.warnings.push(warning);
        }
        return;
    }
    out.passages.push(Passage {
        topic,
        heading: heading.map(str::to_owned),
        records: records.to_vec(),
        text,
        evidence,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::{DocumentKind, Format, PageSource, page};

    fn fixture(raw: &str) -> (Page, Extraction) {
        let source = PageSource {
            repository: None,
            id: "test".into(),
            vendor: "openai".into(),
            kind: DocumentKind::ReleaseBlog,
            url: "https://openai.com/index/model/".into(),
            format: Format::Html,
            records: vec!["model:test".into()],
        };
        let page = Page::parse(&source, raw.as_bytes()).unwrap();
        let out = page::metadata(&source, &page).unwrap();
        (page, out)
    }

    #[test]
    fn keeps_complete_original_paragraph_beyond_the_evidence_preview() {
        let ending = "Published wording. ".repeat(30);
        let (page, mut out) = fixture(&format!(
            "<p>  Model <em>capabilities</em>, with punctuation.\n  {ending}Complete ending.  </p>"
        ));
        let expected = format!("Model capabilities, with punctuation.\n  {ending}Complete ending.");
        push(
            &mut out,
            &page,
            page.select("p")[0],
            PassageTopic::Capabilities,
            Some("Capabilities"),
            &["model:test".into()],
        );
        assert_eq!(out.passages.len(), 1);
        let passage = &out.passages[0];
        assert_eq!(passage.text, expected);
        assert_eq!(passage.heading.as_deref(), Some("Capabilities"));
        assert_eq!(passage.records, ["model:test"]);
        assert_eq!(passage.evidence.excerpt.chars().count(), 240);
        let selector = passage.evidence.locator.strip_prefix("css:").unwrap();
        assert_eq!(original_text(page.select(selector)[0]), expected);
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn skips_duplicate_nodes_and_duplicate_text() {
        let (page, mut out) =
            fixture("<p>Same paragraph.</p><p>Same paragraph.</p><p>Different paragraph.</p>");
        let nodes = page.select("p");
        for node in [nodes[0], nodes[0], nodes[1], nodes[2]] {
            push(&mut out, &page, node, PassageTopic::Overview, None, &[]);
        }
        assert_eq!(out.passages.len(), 2);
        assert_eq!(out.passages[0].text, "Same paragraph.");
        assert_eq!(out.passages[1].text, "Different paragraph.");
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn embedded_pages_with_matching_local_paths_keep_distinct_passages() {
        let (page, mut out) = fixture("<p>Original page paragraph.</p>");
        let (embedded, _) = fixture("<p>Embedded article paragraph.</p>");
        push(
            &mut out,
            &page,
            page.select("p")[0],
            PassageTopic::Overview,
            None,
            &[],
        );
        push(
            &mut out,
            &embedded,
            embedded.select("p")[0],
            PassageTopic::Capabilities,
            None,
            &[],
        );
        assert_eq!(out.passages.len(), 2);
        assert_eq!(out.passages[1].text, "Embedded article paragraph.");
    }

    #[test]
    fn rejects_oversize_paragraphs_without_truncation_or_duplicate_warnings() {
        let (page, mut out) = fixture(&format!(
            "<p>{}</p><p>Complete short paragraph.</p>",
            "模".repeat(2_001)
        ));
        let nodes = page.select("p");
        for node in [nodes[0], nodes[0], nodes[1]] {
            push(&mut out, &page, node, PassageTopic::Overview, None, &[]);
        }
        assert_eq!(out.passages.len(), 1);
        assert_eq!(out.passages[0].text, "Complete short paragraph.");
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].contains("2001 characters exceeds the passage limit of 2000"));
    }

    #[test]
    fn bounds_total_characters_and_passage_count() {
        let raw = (0..4)
            .map(|index| format!("<p>{index}{}</p>", "模".repeat(1_999)))
            .collect::<String>();
        let (page, mut out) = fixture(&raw);
        for node in page.select("p") {
            push(&mut out, &page, node, PassageTopic::Overview, None, &[]);
        }
        assert_eq!(out.passages.len(), 3);
        assert_eq!(
            out.passages
                .iter()
                .map(|passage| passage.text.chars().count())
                .sum::<usize>(),
            6_000
        );
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].contains("page limit of 6000"));

        let raw = (0..7)
            .map(|index| format!("<p>Paragraph {index}.</p>"))
            .collect::<String>();
        let (page, mut out) = fixture(&raw);
        for node in page.select("p") {
            push(&mut out, &page, node, PassageTopic::Overview, None, &[]);
        }
        assert_eq!(out.passages.len(), 6);
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].contains("already has 6 passages"));
    }
}
