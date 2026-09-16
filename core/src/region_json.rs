//! Versioned serialization for the independent owned WASM boundary.
#![forbid(unsafe_code)]
use crate::{
    experiment::{Candidate, Work},
    frame, scan,
};
fn text(d: [u8; 13]) -> String {
    d.iter().map(|v| (b'0' + v) as char).collect()
}
// Writing formatted primitives to String is infallible; append without temporary allocations.
fn work(w: &Work) -> String {
    let s = format!("{{\"paths\":{},\"samples\":{},\"low_contrast\":{},\"run_windows\":{},\"quiet_pass\":{},\"guard_pass\":{},\"run_decoder_calls\":{},\"accepted_paths\":{},\"conflicts\":{},\"continuity_samples\":{},\"continuity_rejects\":{},\"capped_paths\":{},\"profile_boundary_pairs\":{},\"profile_digit_hypotheses\":{},\"truncated_paths\":{},\"retry_paths\":{},\"retry_paths_pending\":{},\"sampling_plan_capped\":{},\"discovery_paths\":{},\"scale_hint_used\":{},\"unresolved_probe_paths\":{},\"sparse_normalizations\":{},\"association_checks\":{},\"association_truncated\":{},\"continuity_capped_links\":{},\"retained_initial_detections\":{},\"cleanup_paths\":{},\"cleanup_examined\":{},\"cleanup_removed_runs\":{},\"cleanup_pixels\":{},\"interior_paths\":{},\"interior_values\":{},\"bias_paths\":{},\"bias_model_pass\":{},\"bias_guard_pass\":{}}}",w.paths,w.samples,w.low_contrast,w.windows,w.quiet_pass,w.guard_pass,w.decoder_calls,w.accepted_paths,w.conflicts,w.continuity_samples,w.continuity_rejects,w.capped_paths,w.profile_boundary_pairs,w.profile_digit_hypotheses,w.truncated_paths,w.retry_paths,w.retry_paths_pending,w.sampling_plan_capped,w.discovery_paths,w.scale_hint_used,w.unresolved_probe_paths,w.sparse_normalizations,w.association_checks,w.association_truncated,w.continuity_capped_links,w.retained_initial_detections,w.cleanup_paths,w.cleanup_examined,w.cleanup_removed_runs,w.cleanup_pixels,w.interior_paths,w.interior_values,w.bias_paths,w.bias_model_pass,w.bias_guard_pass);
    #[cfg(feature = "experimental-invalid-visual-veto")]
    let s = {
        use std::fmt::Write as _;
        let mut s = s;
        s.pop();
        let _ = write!(s, ",\"invalid_visual_seen\":{},\"invalid_veto_intervals\":{},\"invalid_veto_reads\":{},\"invalid_soft_conflicts\":{},\"invalid_veto_capped\":{}}}",w.invalid_visual_seen,w.invalid_veto_intervals,w.invalid_veto_reads,w.invalid_soft_conflicts,w.invalid_veto_capped);
        s
    };
    #[cfg(feature = "experimental-gap-density")]
    let s = {
        use std::fmt::Write as _;
        let mut s = s;
        s.pop();
        let _ = write!(
            s,
            ",\"continuity_cache_hits\":{}}}",
            w.continuity_cache_hits
        );
        s
    };
    #[cfg(feature = "experimental-verified-coverage-reuse")]
    let s = {
        use std::fmt::Write as _;
        let mut s = s;
        s.pop();
        let _ = write!(s, ",\"extension_cache_hits\":{},\"extension_samples\":{},\"extension_claims\":{},\"extension_capped\":{},\"reuse_claims\":{},\"reuse_claims_rejected\":{},\"reuse_checks\":{},\"reuse_checks_capped\":{},\"reuse_paths_changed\":{},\"reuse_paths_removed\":{},\"reuse_paths_split\":{},\"reuse_short_pieces\":{}}}",w.extension_cache_hits,w.extension_samples,w.extension_claims,w.extension_capped,w.reuse_claims,w.reuse_claims_rejected,w.reuse_checks,w.reuse_checks_capped,w.reuse_paths_changed,w.reuse_paths_removed,w.reuse_paths_split,w.reuse_short_pieces);
        s
    };
    #[cfg(feature = "experimental-forward-blur")]
    let s = {
        use std::fmt::Write as _;
        let mut s = s;
        s.pop();
        let _ = write!(s, ",\"forward_blur_calls\":{},\"forward_blur_windows\":{},\"forward_blur_model_attempts\":{},\"forward_blur_accepted_windows\":{},\"forward_blur_conflicts\":{}}}",w.forward_blur_calls,w.forward_blur_windows,w.forward_blur_model_attempts,w.forward_blur_accepted_windows,w.forward_blur_conflicts);
        s
    };
    #[cfg(feature = "experimental-structural-retry")]
    let s = {
        use std::fmt::Write as _;
        let mut s = s;
        s.pop();
        let _ = write!(
            s,
            ",\"max_run_count\":{},\"structural_retry_skipped\":{}}}",
            w.max_run_count, w.structural_retry_skipped
        );
        s
    };
    #[cfg(feature = "experimental-redundant-decode")]
    let s = {
        use std::fmt::Write as _;
        let mut s = s;
        s.pop();
        let _ = write!(
            s,
            ",\"redundant_decode_calls_avoided\":{}}}",
            w.redundant_decode_calls_avoided
        );
        s
    };
    #[cfg(feature = "experimental-extrema-runs")]
    let s = {
        use std::fmt::Write as _;
        let mut s = s;
        s.pop();
        let _ = write!(s, ",\"extrema_calls\":{},\"extrema_examined\":{},\"extrema_capped\":{},\"extrema_ambiguous\":{},\"extrema_decoder_calls\":{}}}",w.extrema_calls,w.extrema_examined,w.extrema_capped,w.extrema_ambiguous,w.extrema_decoder_calls);
        s
    };
    s
}
fn coverage_json(q: scan::Quad) -> String {
    format!(
        "[{}]",
        q.iter()
            .map(|p| format!("[{},{}]", finite_json(p[0]), finite_json(p[1])))
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn finite_json(v: f64) -> String {
    if v.is_finite() {
        v.to_string()
    } else {
        "null".into()
    }
}
#[must_use]
pub fn candidate_json(candidates: &[Candidate]) -> String {
    let cs:Vec<String>=candidates.iter().map(|c|{let ds:Vec<String>=c.detections.iter().map(|d|format!("{{\"text\":\"{}\",\"polygon\":{:?},\"support\":{},\"axis\":{}}}",text(d.digits),d.polygon,d.support,d.axis)).collect();let obs:Vec<String>=c.observations.iter().filter(|_|!cfg!(feature="experimental-compact-output")).map(|d|format!("{{\"text\":\"{}\",\"axis\":{},\"fraction\":{},\"left\":{},\"right\":{},\"cost\":{},\"gap\":{}}}",if d.ambiguous{String::new()}else{text(d.digits)},d.axis,d.fraction,d.left,d.right,d.cost,d.gap)).collect();format!("{{\"candidate_index\":{},\"coverage\":{},\"error\":{},\"error_detail\":{},\"unfinished\":{},\"ms\":{},\"work\":{},\"detections\":[{}],\"observations\":[{}]}}",c.index,coverage_json(c.coverage),c.error,if scan::transform(c.coverage).is_err(){"\"invalid_geometry\""}else if c.error{"\"sampling_error\""}else{"null"},c.error||c.work.invalid_veto_intervals>0||c.work.retry_paths_pending>0||c.work.sampling_plan_capped>0||c.work.truncated_paths>0||c.work.capped_paths>0||c.work.association_truncated>0,c.ms,work(&c.work),ds.join(","),obs.join(","))}).collect();
    cs.join(",")
}
#[must_use]
pub fn frame_json(frame: &frame::Frame) -> String {
    let trace = if cfg!(feature = "experimental-compact-output") {
        format!(
            ",\"diagnostics\":{{\"observationsIncluded\":false,\"observationCount\":{}}}",
            frame
                .candidates
                .iter()
                .map(|c| c.observations.len())
                .sum::<usize>()
        )
    } else {
        String::new()
    };
    let barcodes:Vec<_>=frame.barcodes.iter().map(|b|format!("{{\"text\":\"{}\",\"polygon\":{:?},\"support\":{},\"axis\":{},\"candidate_indices\":{:?}}}",text(b.detection.digits),b.detection.polygon,b.detection.support,b.detection.axis,b.candidate_indices)).collect();
    let w = &frame.reconciliation;
    #[cfg(feature = "experimental-identity-optional")]
    let optional = format!(
        ",\"optional_identity_deferred\":{}",
        w.optional_identity_deferred
    );
    #[cfg(not(feature = "experimental-identity-optional"))]
    let optional = String::new();
    format!("{{\"unfinished\":{}{},\"reconciliation\":{{\"comparisons\":{},\"merged\":{},\"ambiguous\":{},\"conflicting\":{},\"pending_observations\":{},\"truncated\":{},\"source_pairs\":{},\"source_matches\":{},\"source_pixels\":{},\"source_capped\":{},\"pending_coverage_checks\":{},\"pending_quarantined\":{}{}}},\"barcodes\":[{}],\"candidates\":[{}]}}",frame.unfinished,trace,w.comparisons,w.merged,w.ambiguous,w.conflicting,w.pending_observations,w.truncated,w.source_pairs,w.source_matches,w.source_pixels,w.source_capped,w.pending_coverage_checks,w.pending_quarantined,optional,barcodes.join(","),candidate_json(&frame.candidates))
}
