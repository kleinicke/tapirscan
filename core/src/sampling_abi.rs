//! Trusted-host ABI only. Each pointer must originate here, remain live, and
//! have exclusive access. JS wrapper validates inputs and never exposes handles.
use crate::sampling::{image_len, ImageView, Path, Sampler, Transform, PROFILE_LEN};
pub struct Session {
    image: Vec<u8>,
    row_scanner: crate::row_scan::Scanner,
    row_output: Vec<f64>,
    half: crate::pyramid::HalfImage,
    rgba: crate::rgba::RgbaImage,
    neural_input: crate::neural_input::Input,
    associator: crate::band_association::Associator,
    association_reads: [f64; 1280],
    association_parents: [u32; 128],
    shape: (usize, usize, usize, usize),
    matrix: [f64; 9],
    association: [f64; 256],
    sampler: Sampler,
    output: [f32; PROFILE_LEN],
    crop: crate::warp::Warper,
    crop_shape: (usize, usize),
    crop_channels: usize,
    adaptive: crate::preprocess::AdaptiveThreshold,
    enhance: crate::enhance::UpscaleSharpen,
    localizer: crate::localize::Localizer,
    oriented: crate::oriented::Localizer,
    proposals: [f64; 60],
    profile_digits: [u8; 13],
    profile_meta: [f32; 5],
    run_continuity: crate::run_continuity::Correspondence,
    continuity: crate::continuity::Continuity,
    scanner: crate::scan::Scanner,
    bands: crate::bands::BandScanner,
    scan_input: [f64; 512],
    scan_output: [f64; 2048],
}
#[no_mangle]
pub extern "C" fn sampler_new() -> *mut Session {
    Box::into_raw(Box::new(Session {
        image: Vec::new(),
        row_scanner: crate::row_scan::Scanner::default(),
        row_output: Vec::new(),
        half: crate::pyramid::HalfImage::default(),
        rgba: crate::rgba::RgbaImage::default(),
        neural_input: crate::neural_input::Input::default(),
        associator: crate::band_association::Associator::default(),
        association_reads: [0.; 1280],
        association_parents: [0; 128],
        shape: (0, 0, 0, 0),
        matrix: [0.; 9],
        association: [0.; 256],
        sampler: Sampler::default(),
        output: [0.; PROFILE_LEN],
        crop: crate::warp::Warper::default(),
        crop_shape: (0, 0),
        crop_channels: 0,
        adaptive: crate::preprocess::AdaptiveThreshold::default(),
        enhance: crate::enhance::UpscaleSharpen::default(),
        localizer: crate::localize::Localizer::default(),
        oriented: crate::oriented::Localizer::default(),
        proposals: [0.; 60],
        profile_digits: [0; 13],
        profile_meta: [0.; 5],
        run_continuity: crate::run_continuity::Correspondence::default(),
        continuity: crate::continuity::Continuity::default(),
        scanner: crate::scan::Scanner::default(),
        bands: crate::bands::BandScanner::default(),
        scan_input: [0.; 512],
        scan_output: [0.; 2048],
    }))
}
#[no_mangle]
pub unsafe extern "C" fn sampler_resize(
    s: *mut Session,
    w: usize,
    h: usize,
    c: usize,
    stride: usize,
) -> bool {
    let Ok(n) = image_len(w, h, c, stride) else {
        return false;
    };
    let s = &mut *s;
    s.crop_shape = (0, 0);
    if s.image
        .try_reserve(n.saturating_sub(s.image.len()))
        .is_err()
    {
        return false;
    }
    s.image.resize(n, 0);
    s.shape = (w, h, c, stride);
    true
}
#[no_mangle]
pub unsafe extern "C" fn sampler_input(s: *mut Session) -> *mut u8 {
    (*s).image.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_matrix(s: *mut Session) -> *mut f64 {
    (*s).matrix.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_output(s: *mut Session) -> *const f32 {
    (*s).output.as_ptr()
}
/// 1: profile, 0: insufficient contrast, -1: invalid input. Output cleared on rejection.
#[no_mangle]
pub unsafe extern "C" fn sampler_sample(
    s: *mut Session,
    axis: usize,
    fraction: f64,
    curve: f64,
    margin: f64,
) -> i32 {
    let s = &mut *s;
    s.output.fill(0.);
    let (w, h, c, stride) = s.shape;
    let (Ok(im), Ok(m)) = (
        ImageView::new(&s.image, w, h, c, stride),
        Transform::new(s.matrix),
    ) else {
        return -1;
    };
    match s.sampler.sample(
        im,
        m,
        Path {
            axis,
            fraction,
            curve,
            margin,
        },
    ) {
        Ok(Some(p)) => {
            s.output.copy_from_slice(p);
            1
        }
        Ok(None) => 0,
        Err(_) => -1,
    }
}
#[no_mangle]
pub unsafe extern "C" fn sampler_destroy(s: *mut Session) {
    if !s.is_null() {
        drop(Box::from_raw(s));
    }
}

#[no_mangle]
pub unsafe extern "C" fn sampler_warp(s: *mut Session, width: usize, height: usize) -> bool {
    warp_session(&mut *s, width, height, false)
}
#[no_mangle]
pub unsafe extern "C" fn sampler_warp_luminance(
    s: *mut Session,
    width: usize,
    height: usize,
) -> bool {
    warp_session(&mut *s, width, height, true)
}
fn warp_session(s: &mut Session, width: usize, height: usize, gray: bool) -> bool {
    s.crop_shape = (0, 0);
    s.crop_channels = 0;
    let (w, h, c, stride) = s.shape;
    let (Ok(image), Ok(matrix)) = (
        ImageView::new(&s.image, w, h, c, stride),
        Transform::new(s.matrix),
    ) else {
        return false;
    };
    s.crop_shape = (0, 0);
    if (if gray {
        s.crop.warp_luminance(image, matrix, width, height)
    } else {
        s.crop.warp(image, matrix, width, height)
    })
    .is_err()
    {
        return false;
    }
    s.crop_shape = (width, height);
    s.crop_channels = if gray { 1 } else { c };
    true
}
#[no_mangle]
pub unsafe extern "C" fn sampler_crop_output(s: *mut Session) -> *const u8 {
    (*s).crop.bytes().as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn sampler_crop_threshold(
    s: *mut Session,
    block: usize,
    offset: f64,
) -> bool {
    let s = &mut *s;
    let (w, h) = s.crop_shape;
    let c = s.crop_channels;
    let Ok(image) = ImageView::new(s.crop.bytes(), w, h, c, w * c) else {
        return false;
    };
    s.adaptive.process(image, block, offset).is_ok()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_threshold_output(s: *mut Session) -> *const u8 {
    (*s).adaptive.bytes().as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn sampler_association_input(s: *mut Session) -> *mut f64 {
    (*s).association.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_overlap(s: *mut Session, n: usize, m: usize) -> f64 {
    if !(2..=64).contains(&n) || !(2..=64).contains(&m) {
        return -1.;
    }
    let v = &(*s).association;
    let a: Vec<_> = (0..n).map(|i| [v[i * 2], v[i * 2 + 1]]).collect();
    let b: Vec<_> = (0..m).map(|i| [v[128 + i * 2], v[129 + i * 2]]).collect();
    crate::association::overlap(&a, &b).unwrap_or(-1.)
}

#[no_mangle]
pub unsafe extern "C" fn sampler_crop_up2_sharpen(s: *mut Session) -> bool {
    let s = &mut *s;
    let (w, h) = s.crop_shape;
    let c = s.crop_channels;
    let Ok(image) = ImageView::new(s.crop.bytes(), w, h, c, w * c) else {
        return false;
    };
    s.enhance.process(image).is_ok()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_enhance_output(s: *mut Session) -> *const u8 {
    (*s).enhance.bytes().as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn sampler_localize(s: *mut Session) -> i32 {
    let s = &mut *s;
    s.proposals.fill(0.);
    let (w, h, c, stride) = s.shape;
    let Ok(image) = ImageView::new(&s.image, w, h, c, stride) else {
        return -1;
    };
    let Ok(found) = s.localizer.detect(image) else {
        return -1;
    };
    for (i, p) in found.iter().enumerate() {
        s.proposals[i * 5..i * 5 + 4].copy_from_slice(&p.bounds);
        s.proposals[i * 5 + 4] = p.score;
    }
    found.len() as i32
}
#[no_mangle]
pub unsafe extern "C" fn sampler_localize_omitted(s: *mut Session) -> usize {
    (*s).localizer.omitted
}
#[no_mangle]
pub unsafe extern "C" fn sampler_localize_limited(s: *mut Session) -> u32 {
    u32::from((*s).localizer.work_limited)
}
#[no_mangle]
pub unsafe extern "C" fn sampler_proposal_output(s: *mut Session) -> *const f64 {
    (*s).proposals.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn sampler_profile_decode(s: *mut Session) -> i32 {
    let s = &mut *s;
    s.profile_digits.fill(0);
    match crate::profile::decode(&s.output) {
        Ok(Some(r)) => {
            s.profile_digits = r.digits;
            s.profile_meta = [
                r.left,
                r.right,
                r.cost,
                r.gap,
                if r.reversed { 1. } else { 0. },
            ];
            1
        }
        Ok(None) => 0,
        Err(_) => -1,
    }
}
#[no_mangle]
pub unsafe extern "C" fn sampler_profile_digits(s: *mut Session) -> *const u8 {
    (*s).profile_digits.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn sampler_profile_meta(s: *mut Session) -> *const f32 {
    (*s).profile_meta.as_ptr()
}

/// Fixed-size trusted-host batch ABI. Input: up to64 cyclic quads (8f64 each).
/// Output:32f64/region; status, attempts,13digits,8quad coordinates,cost,gap,axis,
/// two fraction/left/right triples. Status -1 invalid,0 undecoded,1 decoded.
#[no_mangle]
pub unsafe extern "C" fn sampler_scan_input(s: *mut Session) -> *mut f64 {
    (*s).scan_input.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_scan_output(s: *mut Session) -> *const f64 {
    (*s).scan_output.as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_scan(s: *mut Session, count: usize) -> i32 {
    sampler_scan_mode(s, count, 0)
}
#[no_mangle]
pub unsafe extern "C" fn sampler_scan_mode(s: *mut Session, count: usize, mode: usize) -> i32 {
    if mode > 2 {
        return -1;
    }
    let s = &mut *s;
    s.scan_output.fill(0.);
    if count > crate::scan::MAX_CANDIDATES {
        return -1;
    }
    let (w, h, c, stride) = s.shape;
    let Ok(image) = ImageView::new(&s.image, w, h, c, stride) else {
        return -1;
    };
    let mut candidates = [[[0.; 2]; 4]; crate::scan::MAX_CANDIDATES];
    for (i, q) in candidates[..count].iter_mut().enumerate() {
        for j in 0..4 {
            q[j] = [s.scan_input[i * 8 + j * 2], s.scan_input[i * 8 + j * 2 + 1]];
        }
    }
    let Ok(results) = (if mode == 2 {
        s.scanner
            .scan_regions_with_runs(image, &candidates[..count])
    } else {
        s.scanner
            .scan_regions_with_contrast(image, &candidates[..count], mode == 1)
    }) else {
        return -1;
    };
    for (i, r) in results.iter().enumerate() {
        let out = &mut s.scan_output[i * 32..(i + 1) * 32];
        out[1] = r.paths_attempted as f64;
        out[0] = if r.error.is_some() { -1. } else { 0. };
        if let Some(read) = r.read {
            out[0] = if read.run_width {
                3.
            } else if read.contrast_normalized {
                2.
            } else {
                1.
            };
            for j in 0..13 {
                out[2 + j] = f64::from(read.digits[j]);
            }
            for j in 0..4 {
                out[15 + j * 2] = read.polygon[j][0];
                out[16 + j * 2] = read.polygon[j][1];
            }
            out[23] = f64::from(read.cost);
            out[24] = f64::from(read.gap);
            out[25] = read.axis as f64;
            for j in 0..2 {
                out[26 + j * 3] = read.paths[j].fraction;
                out[27 + j * 3] = read.paths[j].left;
                out[28 + j * 3] = read.paths[j].right;
            }
        }
    }
    count as i32
}

/// Uses existing association input: two quads at offsets 0 and 128. -1 invalid,
/// 0 unsupported, positive minimum correlation for a continuous stripe corridor.
#[no_mangle]
pub unsafe extern "C" fn sampler_continuity(s: *mut Session) -> f64 {
    let s = &mut *s;
    let (w, h, c, stride) = s.shape;
    let Ok(image) = ImageView::new(&s.image, w, h, c, stride) else {
        return -1.;
    };
    let mut a = [[0.; 2]; 4];
    let mut b = a;
    for i in 0..4 {
        a[i] = [s.association[i * 2], s.association[i * 2 + 1]];
        b[i] = [s.association[128 + i * 2], s.association[129 + i * 2]];
    }
    match s.continuity.check(image, a, b) {
        Ok(e) => {
            if e.supported {
                e.minimum_correlation
            } else {
                0.
            }
        }
        Err(_) => -1.,
    }
}

/// One supplied region, <=8 owned read records in the shared scan output.
/// Output[256]=attempted paths; [257]=truncated flag. No expected-text input.
#[no_mangle]
pub unsafe extern "C" fn sampler_bands(s: *mut Session) -> i32 {
    let s = &mut *s;
    s.scan_output.fill(0.);
    let (w, h, c, stride) = s.shape;
    let Ok(image) = ImageView::new(&s.image, w, h, c, stride) else {
        return -1;
    };
    let mut q = [[0.; 2]; 4];
    for j in 0..4 {
        q[j] = [s.scan_input[j * 2], s.scan_input[j * 2 + 1]];
    }
    let Ok(reads) = s.bands.scan(image, q) else {
        return -1;
    };
    let count = reads.len();
    for (i, read) in reads.iter().enumerate() {
        let out = &mut s.scan_output[i * 32..(i + 1) * 32];
        out[0] = 1.;
        for j in 0..13 {
            out[2 + j] = f64::from(read.digits[j]);
        }
        for j in 0..4 {
            out[15 + j * 2] = read.polygon[j][0];
            out[16 + j * 2] = read.polygon[j][1];
        }
        out[23] = f64::from(read.cost);
        out[24] = f64::from(read.gap);
        out[25] = read.axis as f64;
        for j in 0..2 {
            out[26 + j * 3] = read.paths[j].fraction;
            out[27 + j * 3] = read.paths[j].left;
            out[28 + j * 3] = read.paths[j].right;
        }
    }
    s.scan_output[256] = s.bands.attempts() as f64;
    s.scan_output[257] = if s.bands.truncated() { 1. } else { 0. };
    count as i32
}

#[no_mangle]
pub unsafe extern "C" fn sampler_oriented(s: *mut Session) -> i32 {
    let s = &mut *s;
    s.scan_output.fill(0.);
    let (w, h, c, stride) = s.shape;
    let Ok(image) = ImageView::new(&s.image, w, h, c, stride) else {
        return -1;
    };
    #[cfg(feature = "experimental-classical-orientation")]
    let Ok(found) = s.localizer.oriented_bands(image) else {
        return -1;
    };
    #[cfg(not(feature = "experimental-classical-orientation"))]
    let Ok(found) = s.oriented.detect(image) else {
        return -1;
    };
    for (i, p) in found.iter().enumerate() {
        for j in 0..4 {
            s.scan_output[i * 9 + j * 2] = p.polygon[j][0];
            s.scan_output[i * 9 + j * 2 + 1] = p.polygon[j][1];
        }
        s.scan_output[i * 9 + 8] = p.score;
    }
    found.len() as i32
}

#[no_mangle]
pub unsafe extern "C" fn sampler_half(s: *mut Session) -> bool {
    let s = &mut *s;
    let (w, h, c, stride) = s.shape;
    let Ok(im) = ImageView::new(&s.image, w, h, c, stride) else {
        return false;
    };
    s.half.reduce(im).is_ok()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_half_output(s: *mut Session) -> *const u8 {
    (*s).half.data().as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_half_width(s: *mut Session) -> usize {
    (*s).half.dimensions().0
}
#[no_mangle]
pub unsafe extern "C" fn sampler_half_height(s: *mut Session) -> usize {
    (*s).half.dimensions().1
}

#[no_mangle]
pub unsafe extern "C" fn sampler_band_association_input(s: *mut Session) -> *mut f64 {
    (*s).association_reads.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_association_output(s: *mut Session) -> *const u32 {
    (*s).association_parents.as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_associate(s: *mut Session, count: usize) -> i32 {
    let s = &mut *s;
    if count > 128 {
        return -1;
    }
    let (w, h, c, stride) = s.shape;
    let Ok(im) = ImageView::new(&s.image, w, h, c, stride) else {
        return -1;
    };
    let mut reads = [crate::band_association::Observation {
        value_id: 0,
        polygon: [[0.; 2]; 4],
        is_anchor: false,
    }; 128];
    for (i, r) in reads[..count].iter_mut().enumerate() {
        let v = &s.association_reads[i * 10..i * 10 + 10];
        if !v[0].is_finite()
            || v[0] < 0.
            || v[0] > f64::from(u32::MAX)
            || v[0].fract() != 0.
            || !(v[1] == 0. || v[1] == 1.)
        {
            return -1;
        }
        r.value_id = v[0] as u32;
        r.is_anchor = v[1] == 1.;
        for j in 0..4 {
            r.polygon[j] = [v[2 + j * 2], v[3 + j * 2]];
        }
    }
    let Ok(parents) = s.associator.associate(im, &reads[..count]) else {
        return -1;
    };
    for (i, p) in parents.iter().enumerate() {
        s.association_parents[i] = *p as u32;
    }
    s.associator.checks() as i32
}

#[no_mangle]
pub unsafe extern "C" fn sampler_neural_input(s: *mut Session, size: usize) -> bool {
    let s = &mut *s;
    let (w, h, c, stride) = s.shape;
    let Ok(im) = ImageView::new(&s.image, w, h, c, stride) else {
        return false;
    };
    s.neural_input.prepare(im, size).is_ok()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_neural_output(s: *mut Session) -> *const f32 {
    (*s).neural_input.data().as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn sampler_row_scan(s: *mut Session) -> isize {
    let s = &mut *s;
    s.row_output.clear();
    let (w, h, c, stride) = s.shape;
    let Ok(image) = ImageView::new(&s.image, w, h, c, stride) else {
        return -1;
    };
    if s.row_scanner.scan(image).is_err() {
        return -1;
    }
    let count = s.row_scanner.results().len();
    let n = 4 + 24 * count;
    if s.row_output.try_reserve(n).is_err() {
        return -1;
    }
    s.row_output.resize(n, 0.);
    s.row_output[0] = count as f64;
    s.row_output[1] = s.row_scanner.rows as f64;
    s.row_output[2] = s.row_scanner.hypotheses as f64;
    s.row_output[3] = f64::from(u8::from(s.row_scanner.truncated));
    for (i, r) in s.row_scanner.results().iter().enumerate() {
        let out = &mut s.row_output[4 + i * 24..4 + (i + 1) * 24];
        if let Some(d) = r.digits {
            out[0] = 1.;
            for j in 0..13 {
                out[1 + j] = f64::from(d[j]);
            }
        }
        for j in 0..8 {
            out[14 + j] = r.polygon[j / 2][j % 2];
        }
        out[22] = r.support as f64;
    }
    n as isize
}
#[no_mangle]
pub unsafe extern "C" fn sampler_row_output(s: *const Session) -> *const f64 {
    (*s).row_output.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn sampler_row_scan_grouped(s: *mut Session) -> isize {
    row_scan_grouped_mode(s, false)
}
unsafe fn row_scan_grouped_mode(s: *mut Session, soft: bool) -> isize {
    let s = &mut *s;
    s.row_output.clear();
    let (w, h, c, stride) = s.shape;
    let Ok(image) = ImageView::new(&s.image, w, h, c, stride) else {
        return -1;
    };
    let Ok(results) = (if soft {
        s.row_scanner.scan_soft_grouped(image)
    } else {
        s.row_scanner.scan_grouped(image)
    }) else {
        return -1;
    };
    let count = results.len();
    let n = 4 + 24 * count;
    if s.row_output.try_reserve(n).is_err() {
        return -1;
    }
    s.row_output.resize(n, 0.);
    for (i, r) in results.iter().enumerate() {
        let out = &mut s.row_output[4 + i * 24..4 + (i + 1) * 24];
        if let Some(d) = r.digits {
            out[0] = 1.;
            for j in 0..13 {
                out[1 + j] = f64::from(d[j]);
            }
        }
        for j in 0..8 {
            out[14 + j] = r.polygon[j / 2][j % 2];
        }
        out[22] = r.support as f64;
        out[23] = r.fragments as f64;
    }
    s.row_output[0] = count as f64;
    s.row_output[1] = s.row_scanner.rows as f64;
    s.row_output[2] = s.row_scanner.hypotheses as f64;
    s.row_output[3] = f64::from(u8::from(s.row_scanner.truncated));
    n as isize
}
#[no_mangle]
pub unsafe extern "C" fn sampler_row_scan_soft(s: *mut Session) -> isize {
    row_scan_grouped_mode(s, true)
}
#[no_mangle]
pub unsafe extern "C" fn sampler_row_group_checks(s: *const Session) -> usize {
    (*s).row_scanner.grouping_stats().0
}
#[no_mangle]
pub unsafe extern "C" fn sampler_row_group_merged(s: *const Session) -> usize {
    (*s).row_scanner.grouping_stats().1
}
#[no_mangle]
pub unsafe extern "C" fn sampler_row_group_exhausted(s: *const Session) -> bool {
    (*s).row_scanner.grouping_stats().2
}

#[no_mangle]
pub unsafe extern "C" fn sampler_run_continuity(s: *mut Session) -> i32 {
    let s = &mut *s;
    let (w, h, c, stride) = s.shape;
    let Ok(im) = ImageView::new(&s.image, w, h, c, stride) else {
        return -1;
    };
    let a = std::array::from_fn(|j| [s.association[j * 2], s.association[j * 2 + 1]]);
    let b = std::array::from_fn(|j| [s.association[128 + j * 2], s.association[129 + j * 2]]);
    let Ok(e) = s.run_continuity.check(im, a, b) else {
        return -1;
    };
    if let Some(d) = e.digits {
        s.profile_digits = d;
    }
    s.profile_meta[0] = e.profiles as f32;
    s.profile_meta[1] = f32::from(e.rejection as u8);
    i32::from(e.supported)
}

#[no_mangle]
pub unsafe extern "C" fn sampler_rgba(s: *mut Session) -> bool {
    let s = &mut *s;
    let (w, h, c, stride) = s.shape;
    let Ok(im) = ImageView::new(&s.image, w, h, c, stride) else {
        return false;
    };
    s.rgba.prepare(im).is_ok()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_rgba_output(s: *mut Session) -> *const u8 {
    (*s).rgba.data().as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn sampler_rgba_width(s: *mut Session) -> usize {
    (*s).rgba.dimensions().0
}
#[no_mangle]
pub unsafe extern "C" fn sampler_rgba_height(s: *mut Session) -> usize {
    (*s).rgba.dimensions().1
}

#[cfg(feature = "experimental-orientation-stripes")]
#[no_mangle]
pub unsafe extern "C" fn sampler_stripes(s: *mut Session) -> i32 {
    let s = &mut *s;
    s.scan_output.fill(0.);
    let (w, h, c, stride) = s.shape;
    let Ok(image) = ImageView::new(&s.image, w, h, c, stride) else {
        return -1;
    };
    let Ok(found) = crate::stripes::detect(image) else {
        return -1;
    };
    s.localizer.omitted = found.omitted;
    s.localizer.work_limited = found.limited;
    for (i, p) in found.proposals.iter().enumerate() {
        for j in 0..4 {
            s.scan_output[i * 9 + j * 2] = p.polygon[j][0];
            s.scan_output[i * 9 + j * 2 + 1] = p.polygon[j][1];
        }
        s.scan_output[i * 9 + 8] = p.score;
    }
    for (i, n) in found.trace.iter().enumerate() {
        s.scan_output[256 + i] = *n as f64;
    }
    found.proposals.len() as i32
}
