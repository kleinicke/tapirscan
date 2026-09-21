use super::Result;

pub(super) fn to_json(result: &Result, mode: &str, elapsed_ms: f64) -> String {
    assert!(["low", "medium", "high", "very-high"].contains(&mode));
    assert!(elapsed_ms.is_finite() && elapsed_ms >= 0.0);
    let (details, frame) = if result.options.include_regions {
        detailed_frame(result)
    } else {
        compact_frame(result)
    };
    let recovery = if result.options.include_regions {
        result
            .recovery
            .as_ref()
            .map(|value| format!(",\"recovery\":{value}"))
            .unwrap_or_default()
    } else {
        String::new()
    };
    format!(
        "{{\"schemaVersion\":2,\"mode\":\"{}\",\"multiple\":{},\"elapsedMs\":{},\"localizationLimited\":{}{},\"scan\":{}}}",
        mode,
        result.options.multiple,
        elapsed_ms,
        result.localization_work_limited,
        format_args!("{details}{recovery}"),
        frame
    )
}

fn detailed_frame(result: &Result) -> (String, String) {
    let proposals = result
        .proposals
        .iter()
        .map(|proposal| {
            format!(
                "{{\"polygon\":{:?},\"score\":{},\"text\":\"\"}}",
                proposal.polygon, proposal.score
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    (
        format!(
            ",\"localization\":{{\"proposals\":[{}],\"omitted\":{},\"workLimited\":{}}},\"searchWindows\":[{{\"kind\":\"full_frame_search\",\"polygon\":{:?},\"candidateIndex\":{}}}]",
            proposals,
            result.localization_omitted,
            result.localization_work_limited,
            result.search_window,
            result.proposals.len()
        ),
        barcode_research_core::region_json::frame_json(&result.scan.frame),
    )
}

fn compact_frame(result: &Result) -> (String, String) {
    let reads = result
        .barcodes()
        .iter()
        .map(|barcode| {
            let detection = &barcode.detection;
            let text: String = detection
                .digits
                .iter()
                .map(|value| (b'0' + value) as char)
                .collect();
            format!(
                "{{\"text\":\"{}\",\"polygon\":{:?},\"support\":{},\"axis\":{},\"candidate_indices\":{:?}}}",
                text,
                detection.polygon,
                detection.support,
                detection.axis,
                barcode.candidate_indices
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    (
        String::new(),
        format!(
            "{{\"unfinished\":{},\"barcodes\":[{}]}}",
            result.unfinished(),
            reads
        ),
    )
}
