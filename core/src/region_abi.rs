//! Owned, single-threaded WASM host boundary. Handles are monotonic IDs, never
//! raw ownership pointers. A destroyed ID is never reused. Host views expire on
//! prepare/scan/destroy or memory growth; callers must copy returned JSON.
// Exported symbol names require the unsafe_code lint allowance; all bodies
// use safe Rust and never dereference host-provided pointers.
#![deny(unsafe_op_in_unsafe_fn)]
use crate::{
    region_scan::{ImageView, Policy, RegionScanner},
    sampling::image_len,
};

use std::fmt::Write;
use std::{cell::RefCell, collections::BTreeMap};
#[derive(Default)]
struct State {
    engine: RegionScanner,
    input: Vec<u8>,
    quads: Vec<f64>,
    output: Vec<u8>,
    shape: Option<(usize, usize, usize, usize)>,
}
struct Registry {
    next: u32,
    states: BTreeMap<u32, State>,
}
thread_local! {static REGISTRY:RefCell<Registry>=const { RefCell::new(Registry{next:1,states:BTreeMap::new()}) };}
/// 0 success,1 invalid handle,2 image parameters,3 allocation,4 not prepared,
/// 5 scanner policy/count,6 internal image validation. New returns0 on capacity.
#[no_mangle]
pub extern "C" fn regions_new() -> u32 {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        if r.states.len() >= 4 || r.next == u32::MAX {
            return 0;
        }
        let id = r.next;
        r.next += 1;
        r.states.insert(id, State::default());
        id
    })
}
#[no_mangle]
pub extern "C" fn regions_destroy(id: u32) -> u32 {
    REGISTRY.with(|r| u32::from(r.borrow_mut().states.remove(&id).is_none()))
}
#[no_mangle]
pub extern "C" fn regions_prepare(
    id: u32,
    width: usize,
    height: usize,
    c: usize,
    stride: usize,
) -> u32 {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let Some(s) = r.states.get_mut(&id) else {
            return 1;
        };
        s.output.clear();
        s.shape = None;

        let Ok(count) = image_len(width, height, c, stride) else {
            return 2;
        };
        if count > s.input.len() && s.input.try_reserve_exact(count - s.input.len()).is_err() {
            return 3;
        }
        if s.quads
            .try_reserve_exact(512usize.saturating_sub(s.quads.len()))
            .is_err()
        {
            return 3;
        }
        s.input.resize(count, 0);
        s.quads.resize(512, 0.);
        s.quads.fill(0.);
        s.shape = Some((width, height, c, stride));
        0
    })
}
#[no_mangle]
pub extern "C" fn regions_input_ptr(id: u32) -> usize {
    REGISTRY.with(|r| {
        r.borrow()
            .states
            .get(&id)
            .filter(|s| s.shape.is_some())
            .map_or(0, |s| s.input.as_ptr() as usize)
    })
}
#[no_mangle]
pub extern "C" fn regions_input_len(id: u32) -> usize {
    REGISTRY.with(|r| {
        r.borrow()
            .states
            .get(&id)
            .filter(|s| s.shape.is_some())
            .map_or(0, |s| s.input.len())
    })
}
#[no_mangle]
pub extern "C" fn regions_quads_ptr(id: u32) -> usize {
    REGISTRY.with(|r| {
        r.borrow()
            .states
            .get(&id)
            .filter(|s| s.shape.is_some())
            .map_or(0, |s| s.quads.as_ptr() as usize)
    })
}
#[no_mangle]
pub extern "C" fn regions_output_ptr(id: u32) -> usize {
    REGISTRY.with(|r| {
        r.borrow()
            .states
            .get(&id)
            .map_or(0, |s| s.output.as_ptr() as usize)
    })
}
#[no_mangle]
pub extern "C" fn regions_output_len(id: u32) -> usize {
    REGISTRY.with(|r| r.borrow().states.get(&id).map_or(0, |s| s.output.len()))
}
#[no_mangle]
pub extern "C" fn regions_scan(
    id: u32,
    count: usize,
    flags: u32,
    per_candidate: usize,
    per_frame: usize,
    checks: usize,
    pixels: usize,
    results: usize,
) -> u32 {
    {
        #[cfg(feature = "mode-low")]
        {
            REGISTRY.with(|r| {
                let mut r = r.borrow_mut();
                let Some(s) = r.states.get_mut(&id) else {
                    return 1;
                };
                s.output.clear();
                let Some((width, height, c, stride)) = s.shape else {
                    return 4;
                };
                if count > 64 || flags > 63 {
                    return 5;
                }
                let policy = Policy {
                    complete: flags & 32 != 0,
                    max_retry_paths_per_candidate: per_candidate,
                    max_retry_paths_per_frame: per_frame,
                    max_association_checks: checks,
                    max_association_pixels: pixels,
                    max_results: results,
                    transition_cleanup: flags & 1 != 0,
                    source_identity: flags & 2 != 0,
                    interior_normalization: flags & 4 != 0,
                    guard_bias: flags & 8 != 0,
                    allow_single_row: flags & 16 != 0,
                };
                let Ok(im) = ImageView::new(&s.input, width, height, c, stride) else {
                    return 6;
                };
                let qs: Vec<[[f64; 2]; 4]> = s.quads[..count * 8]
                    .chunks_exact(8)
                    .map(|quad| std::array::from_fn(|i| [quad[i * 2], quad[i * 2 + 1]]))
                    .collect();
                s.engine.engine.retail.reset();
                let Ok(result) = s.engine.scan(im, &qs, policy) else {
                    return 5;
                };
                s.engine.engine.retail.set_primary(&result.frame);

                {
                    s.output = crate::region_json::frame_json(&result.frame).into_bytes();
                }
                if s.engine.engine.retail.mask & 12 != 0 {
                    let extra = s.engine.engine.retail.finish(im);
                    s.output.pop();
                    s.output
                        .extend_from_slice(format!(",\"retail\":{extra}}}").as_bytes());
                }
                0
            })
        }
        #[cfg(any(
            feature = "mode-medium",
            feature = "mode-high",
            feature = "mode-very-high"
        ))]
        {
            regions_scan_mask(
                id,
                count,
                flags,
                per_candidate,
                per_frame,
                checks,
                pixels,
                results,
                u32::MAX,
                u32::MAX,
            )
        }
    }
}
#[cfg(any(
    feature = "mode-medium",
    feature = "mode-high",
    feature = "mode-very-high"
))]
/// Research-only region retry scheduling; the fixed/discovery passes still run everywhere.
#[no_mangle]
pub extern "C" fn regions_scan_mask(
    id: u32,
    count: usize,
    flags: u32,
    per_candidate: usize,
    per_frame: usize,
    checks: usize,
    pixels: usize,
    results: usize,
    mask_low: u32,
    mask_high: u32,
) -> u32 {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let Some(s) = r.states.get_mut(&id) else {
            return 1;
        };
        s.output.clear();
        let Some((width, height, c, stride)) = s.shape else {
            return 4;
        };
        if count > 64 || flags > 63 {
            return 5;
        }
        let policy = Policy {
            candidate_retry_mask: u64::from(mask_low) | (u64::from(mask_high) << 32),
            complete: flags & 32 != 0,
            max_retry_paths_per_candidate: per_candidate,
            max_retry_paths_per_frame: per_frame,
            max_association_checks: checks,
            max_association_pixels: pixels,
            max_results: results,
            transition_cleanup: flags & 1 != 0,
            source_identity: flags & 2 != 0,
            interior_normalization: flags & 4 != 0,
            guard_bias: flags & 8 != 0,
            allow_single_row: flags & 16 != 0,
        };
        let Ok(im) = ImageView::new(&s.input, width, height, c, stride) else {
            return 6;
        };
        let qs: Vec<[[f64; 2]; 4]> = s.quads[..count * 8]
            .chunks_exact(8)
            .map(|quad| std::array::from_fn(|i| [quad[i * 2], quad[i * 2 + 1]]))
            .collect();
        #[cfg(feature = "mode-medium")]
        {
            s.engine.engine.retail.reset();
        }
        let Ok(result) = s.engine.scan(im, &qs, policy) else {
            return 5;
        };
        #[cfg(feature = "mode-medium")]
        {
            s.engine.engine.retail.set_primary(&result.frame);
        }

        {
            s.output = crate::region_json::frame_json(&result.frame).into_bytes();
        }
        #[cfg(feature = "mode-medium")]
        {
            if s.engine.engine.retail.mask & 12 != 0 {
                let extra = s.engine.engine.retail.finish(im);
                s.output.pop();
                s.output
                    .extend_from_slice(format!(",\"retail\":{extra}}}").as_bytes());
            }
        }
        0
    })
}
#[no_mangle]
pub extern "C" fn regions_version() -> u32 {
    1
}
#[cfg(any(
    feature = "mode-low",
    feature = "mode-medium",
    feature = "mode-very-high"
))]
/// Localize the already-owned source buffer. No second image allocation/copy.
/// Returned JSON is invalidated by the next operation on this handle.
#[no_mangle]
#[expect(
    clippy::too_many_lines,
    reason = "The ABI transaction validates its handle, borrows session storage, localizes and serializes before releasing the borrow."
)]
pub extern "C" fn regions_localize(id: u32, fit_limit: usize) -> u32 {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let Some(s) = r.states.get_mut(&id) else {
            return 1;
        };
        s.output.clear();
        if fit_limit > 8 {
            return 5;
        }
        let Some((width, height, c, stride)) = s.shape else {
            return 4;
        };
        let Ok(im) = ImageView::new(&s.input, width, height, c, stride) else {
            return 6;
        };
        let Ok(found) = crate::stripes::detect(im) else {
            return 6;
        };
        let count = found.proposals.len();
        let examined = count.min(fit_limit);
        let mut proposals = found.proposals;
        for i in 0..examined {
            if let Some(p) = crate::shear::refine(im, proposals[i].polygon) {
                proposals.push(p);
            }
        }
        let added = proposals.len() - count;
        // At most eight alternate base proposals plus four shear refinements. Keep
        // the entire original prefix; no code value or GT selects this extra grid.
        let (mut secondary_count, mut secondary_added, mut secondary_omitted) =
            (0usize, 0usize, 0usize);
        let mut secondary_limited = false;
        if cfg!(feature = "mode-very-high") && width.max(height) > 640 {
            if let Ok(second) = crate::stripes::detect_secondary(im) {
                secondary_count = second.proposals.len();
                secondary_limited = second.limited;
                let selected = secondary_count.min(8);
                secondary_omitted = second.omitted + secondary_count - selected;
                let extra: Vec<_> = second.proposals.into_iter().take(selected).collect();
                for p in &extra {
                    proposals.push(crate::oriented::Proposal {
                        polygon: p.polygon,
                        score: p.score,
                    });
                    secondary_added += 1;
                }
                for p in extra.iter().take(4) {
                    if let Some(p) = crate::shear::refine(im, p.polygon) {
                        proposals.push(p);
                        secondary_added += 1;
                    }
                }
                secondary_omitted += selected.saturating_sub(4);
            } else {
                secondary_limited = true;
            }
        }
        debug_assert!(proposals.len() <= 52);

        let mut out = String::from("{\"proposals\":[");
        for (i, p) in proposals.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write!(
                &mut out,
                "{{\"polygon\":{:?},\"text\":\"\",\"score\":{}}}",
                p.polygon, p.score
            )
            .unwrap();
        }
        write!(
            &mut out,
            "],\"omitted\":{},\"workLimited\":{},\"trace\":{{",
            found.omitted + count - examined + secondary_omitted,
            found.limited || count > examined || secondary_limited || secondary_omitted > 0
        )
        .unwrap();
        let names = [
            "originalComponents",
            "mergeEligible",
            "mergedHypotheses",
            "mergeChecks",
            "refinedHypotheses",
            "unrefinedHypotheses",
            "validBeforeCap",
            "extentUnexamined",
            "extentEligible",
            "extentExamined",
            "extentIneligible",
            "bandRefined",
            "bandUnexamined",
        ];
        for (i, n) in found.trace.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write!(&mut out, "\"{}\":{}", names[i], n).unwrap();
        }
        write!(
            &mut out,
            ",\"shearExamined\":{},\"shearAdded\":{},\"shearUnexamined\":{}}}}}",
            examined,
            added,
            count - examined
        )
        .unwrap();
        if cfg!(feature = "mode-very-high") {
            out.pop();
            out.pop();
            write!(
                &mut out,
                ",\"secondaryProposals\":{secondary_count},\"secondaryAdded\":{secondary_added},\"secondaryOmitted\":{secondary_omitted}}}}}"
            )
            .unwrap();
        }
        s.output = out.into_bytes();
        0
    })
}
#[cfg(feature = "mode-high")]
/// Returned JSON is invalidated by the next operation on this handle.
#[no_mangle]
#[expect(
    clippy::too_many_lines,
    reason = "The ABI transaction validates its handle, borrows session storage, localizes and serializes before releasing the borrow."
)]
pub extern "C" fn regions_localize(id: u32, fit_limit: usize) -> u32 {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let Some(s) = r.states.get_mut(&id) else {
            return 1;
        };
        s.output.clear();
        if fit_limit > 8 {
            return 5;
        }
        let Some((width, height, c, stride)) = s.shape else {
            return 4;
        };
        let Ok(im) = ImageView::new(&s.input, width, height, c, stride) else {
            return 6;
        };
        let Ok(found) = crate::stripes::detect(im) else {
            return 6;
        };
        let count = found.proposals.len();
        let examined = count.min(fit_limit);
        let mut proposals = found.proposals;
        for i in 0..examined {
            if let Some(p) = crate::shear::refine(im, proposals[i].polygon) {
                proposals.push(p);
            }
        }
        let added = proposals.len() - count;
        // At most eight alternate base proposals plus four shear refinements. Keep
        // the entire original prefix; no code value or GT selects this extra grid.
        let (mut secondary_count, mut secondary_added, mut secondary_omitted) =
            (0usize, 0usize, 0usize);
        let mut secondary_limited = false;
        if cfg!(feature = "mode-very-high") && width.max(height) > 640 {
            if let Ok(second) = crate::stripes::detect_secondary(im) {
                secondary_count = second.proposals.len();
                secondary_limited = second.limited;
                let selected = secondary_count.min(8);
                secondary_omitted = second.omitted + secondary_count - selected;
                let extra: Vec<_> = second.proposals.into_iter().take(selected).collect();
                for p in &extra {
                    proposals.push(crate::oriented::Proposal {
                        polygon: p.polygon,
                        score: p.score,
                    });
                    secondary_added += 1;
                }
                for p in extra.iter().take(4) {
                    if let Some(p) = crate::shear::refine(im, p.polygon) {
                        proposals.push(p);
                        secondary_added += 1;
                    }
                }
                secondary_omitted += selected.saturating_sub(4);
            } else {
                secondary_limited = true;
            }
        }
        debug_assert!(proposals.len() <= 52);

        let mut out = String::from("{\"proposals\":[");
        for (i, p) in proposals.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write!(
                &mut out,
                "{{\"polygon\":{:?},\"text\":\"\",\"score\":{}}}",
                p.polygon, p.score
            )
            .unwrap();
        }
        write!(
            &mut out,
            "],\"omitted\":{},\"workLimited\":{},\"trace\":{{",
            found.omitted + count - examined + secondary_omitted,
            found.limited || count > examined || secondary_limited || secondary_omitted > 0
        )
        .unwrap();
        let names = [
            "originalComponents",
            "mergeEligible",
            "mergedHypotheses",
            "mergeChecks",
            "refinedHypotheses",
            "unrefinedHypotheses",
            "validBeforeCap",
            "extentUnexamined",
            "extentEligible",
            "extentExamined",
            "extentIneligible",
            "bandRefined",
            "bandUnexamined",
        ];
        for (i, n) in found.trace.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write!(&mut out, "\"{}\":{}", names[i], n).unwrap();
        }
        write!(
            &mut out,
            ",\"shearExamined\":{},\"shearAdded\":{},\"shearUnexamined\":{}}}}}",
            examined,
            added,
            count - examined
        )
        .unwrap();
        if cfg!(feature = "mode-very-high") {
            out.pop();
            out.pop();
            write!(
                &mut out,
                ",\"secondaryProposals\":{secondary_count},\"secondaryAdded\":{secondary_added},\"secondaryOmitted\":{secondary_omitted}}}}}"
            )
            .unwrap();
        }
        s.output = out.into_bytes();
        0
    })
}
#[cfg(feature = "mode-high")]
#[cfg(test)]
mod shared_image_tests {
    use super::*;
    #[test]
    fn localization_owns_one_input_and_does_not_invalidate_decoder_pixels() {
        let id = regions_new();
        assert_eq!(regions_localize(id, 8), 4);
        assert_eq!(regions_prepare(id, 100, 70, 3, 303), 0);
        REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            for (i, p) in r.states.get_mut(&id).unwrap().input.iter_mut().enumerate() {
                *p = ((i * 73) % 256).to_le_bytes()[0];
            }
        });
        let before = REGISTRY.with(|r| r.borrow().states[&id].input.clone());
        let ptr = regions_input_ptr(id);
        assert_eq!(regions_localize(id, 8), 0);
        assert!(regions_output_len(id) > 0);
        assert_eq!(regions_input_ptr(id), ptr);
        assert_eq!(
            REGISTRY.with(|r| r.borrow().states[&id].input.clone()),
            before
        );
        assert_eq!(regions_scan(id, 0, 0, 0, 0, 0, 0, 1), 0);
        assert_eq!(
            REGISTRY.with(|r| r.borrow().states[&id].input.clone()),
            before
        );
        assert_eq!(regions_localize(id, 9), 5);
        assert_eq!(regions_output_len(id), 0);
        assert_eq!(regions_destroy(id), 0);
        assert_eq!(regions_localize(id, 8), 1);
    }
}

// Default ABI compatibility: localization requires the stripe recipe.

/// Capability check for completing effort-selected work (scan flag 32).
#[no_mangle]
pub extern "C" fn regions_completion_supported() -> u32 {
    1
}
#[cfg(any(
    feature = "mode-low",
    feature = "mode-medium",
    feature = "mode-very-high"
))]
#[cfg(test)]
mod shared_image_tests {
    use super::*;
    #[test]
    fn localization_owns_one_input_and_does_not_invalidate_decoder_pixels() {
        let id = regions_new();
        assert_eq!(regions_localize(id, 8), 4);
        assert_eq!(regions_prepare(id, 100, 70, 3, 303), 0);
        REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            for (i, p) in r.states.get_mut(&id).unwrap().input.iter_mut().enumerate() {
                *p = ((i * 73) % 256).to_le_bytes()[0];
            }
        });
        let before = REGISTRY.with(|r| r.borrow().states[&id].input.clone());
        let ptr = regions_input_ptr(id);
        assert_eq!(regions_localize(id, 8), 0);
        assert!(regions_output_len(id) > 0);
        assert_eq!(regions_input_ptr(id), ptr);
        assert_eq!(
            REGISTRY.with(|r| r.borrow().states[&id].input.clone()),
            before
        );
        assert_eq!(regions_scan(id, 0, 0, 0, 0, 0, 0, 1), 0);
        assert_eq!(
            REGISTRY.with(|r| r.borrow().states[&id].input.clone()),
            before
        );
        assert_eq!(regions_localize(id, 9), 5);
        assert_eq!(regions_output_len(id), 0);
        assert_eq!(regions_destroy(id), 0);
        assert_eq!(regions_localize(id, 8), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handle_lifetime_failures_and_stale_output() {
        let id = regions_new();
        assert_ne!(id, 0);
        assert_eq!(regions_scan(id, 0, 0, 0, 0, 0, 0, 1), 4);
        assert_eq!(regions_prepare(id, 100, 100, 1, 100), 0);
        assert_eq!(regions_input_len(id), 10000);
        assert_eq!(regions_scan(id, 0, 0, 0, 0, 0, 0, 1), 0);
        assert!(regions_output_len(id) > 0);
        assert_eq!(regions_scan(id, 0, 16, 0, 0, 0, 0, 1), 0);
        assert_eq!(regions_scan(id, 0, 31, 0, 0, 0, 0, 1), 0);
        assert_eq!(regions_scan(id, 0, 64, 0, 0, 0, 0, 1), 5);
        assert_eq!(regions_output_len(id), 0);

        assert_eq!(regions_scan(id, 65, 0, 0, 0, 0, 0, 1), 5);
        assert_eq!(regions_output_len(id), 0);
        assert_eq!(regions_prepare(id, usize::MAX, 2, 4, usize::MAX), 2);
        assert_eq!(regions_input_len(id), 0);
        assert_eq!(regions_destroy(id), 0);
        assert_eq!(regions_destroy(id), 1);
        assert_eq!(regions_prepare(id, 1, 1, 1, 1), 1);
        let next = regions_new();
        assert_ne!(next, id);
        assert_eq!(regions_destroy(next), 0);
    }
}
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
#[no_mangle]
pub extern "C" fn regions_retail(id: u32, mask: u32, raw: u32) -> u32 {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let Some(s) = r.states.get_mut(&id) else {
            return 1;
        };
        if mask & 1 == 0 || mask & !15 != 0 || raw > 3 {
            return 5;
        }
        s.engine.engine.retail.mask = mask;
        s.engine.engine.retail.use_raw = raw != 0;
        s.engine.engine.retail.use_peaks = raw >= 2;
        s.engine.engine.retail.peak_retry = raw == 3;
        0
    })
}
