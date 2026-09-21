use crate::{Error, Result as ScanResult};
use serde_json::{json, Value};

/// Build the shared schema without serializing and reparsing the complete result.
/// Detailed frame evidence remains owned by the pinned core serializer because its
/// feature-dependent metadata is intentionally opaque to this facade.
pub(crate) fn value(
    result: &ScanResult,
    mode: &str,
    elapsed_ms: f64,
) -> std::result::Result<Value, Error> {
    if !["low", "medium", "high", "very-high"].contains(&mode)
        || !elapsed_ms.is_finite()
        || elapsed_ms < 0.0
    {
        return Err(Error::Parameters);
    }
    let scan = if result.options.include_regions {
        serde_json::from_str(&barcode_research_core::region_json::frame_json(
            &result.scan.frame,
        ))
        .map_err(|_| Error::OutputShape)?
    } else {
        compact_scan(result)
    };
    let mut output = json!({
        "schemaVersion": 2,
        "mode": mode,
        "multiple": result.options.multiple,
        "elapsedMs": elapsed_ms,
        "localizationLimited": result.localization_work_limited,
        "scan": scan,
    });
    if result.options.include_regions {
        output["localization"] = json!({
            "proposals": result.proposals.iter().map(|proposal| json!({
                "polygon": proposal.polygon,
                "score": proposal.score,
                "text": "",
            })).collect::<Vec<_>>(),
            "omitted": result.localization_omitted,
            "workLimited": result.localization_work_limited,
        });
        output["searchWindows"] = json!([{
            "kind": "full_frame_search",
            "polygon": result.search_window,
            "candidateIndex": result.proposals.len(),
        }]);
        if let Some(recovery) = &result.recovery {
            output["recovery"] = recovery.clone();
        }
    }
    Ok(output)
}

fn compact_scan(result: &ScanResult) -> Value {
    json!({
        "unfinished": result.unfinished(),
        "barcodes": result.barcodes().iter().map(|barcode| {
            let detection = &barcode.detection;
            let text: String = detection.digits.iter().map(|value| (b'0' + value) as char).collect();
            json!({
                "text": text,
                "polygon": detection.polygon,
                "support": detection.support,
                "axis": detection.axis,
                "candidate_indices": barcode.candidate_indices,
            })
        }).collect::<Vec<_>>(),
    })
}
