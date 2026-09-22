//! Version 3 native ABI. Handles are registry IDs, never dereferenced pointers.
//! Caller-owned pointer ranges must be valid, correctly aligned and nonoverlapping.
#[cfg(not(target_pointer_width = "64"))]
compile_error!("Native ABI v4 currently supports 64-bit targets only");
use std::{
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex, OnceLock},
};
use tapirscan::{EanAddOnPolicy, Formats, Image, Mode, ScanOptions, Scanner, ScannerOptions};
const ARG: i32 = 1;
const HANDLE: i32 = 2;
const BUFFER: i32 = 3;
const PANIC: i32 = 4;
const CAPACITY: i32 = 5;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BarcodeRead {
    pub polygon: [f64; 8],
    pub support: u32,
    pub text_length: u64,
    pub format: [u8; 24],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ResultInfo {
    pub json_length: u64,
    pub barcode_count: u64,
    pub unfinished: u32,
    pub localization_limited: u32,
}
struct Output {
    json: Vec<u8>,
    reads: Vec<BarcodeRead>,
    texts: Vec<Vec<u8>>,
    unfinished: bool,
    limited: bool,
}
#[derive(Default)]
struct Registry {
    next: u64,
    scanners: HashMap<u64, Arc<Mutex<Scanner>>>,
    results: HashMap<u64, Arc<Output>>,
}
impl Registry {
    fn id(&mut self) -> Result<u64, i32> {
        self.next = self.next.checked_add(1).ok_or(CAPACITY)?;
        Ok(self.next)
    }
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
fn registry() -> Result<std::sync::MutexGuard<'static, Registry>, i32> {
    REGISTRY
        .get_or_init(|| Mutex::new(Registry::default()))
        .lock()
        .map_err(|_| PANIC)
}
fn boundary(f: impl FnOnce() -> Result<(), i32>) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => 0,
        Ok(Err(code)) => code,
        Err(_) => PANIC,
    }
}
fn output(id: u64) -> Result<Arc<Output>, i32> {
    registry()?.results.get(&id).cloned().ok_or(HANDLE)
}
fn scan_output(
    scanner: &Mutex<Scanner>,
    image: Image<'_>,
    flags: u32,
    formats: Formats,
) -> Result<Output, i32> {
    let add_on_policy = match flags & 12 {
        4 => EanAddOnPolicy::Read,
        8 => EanAddOnPolicy::Require,
        _ => EanAddOnPolicy::Ignore,
    };
    let mut scanner = scanner.lock().map_err(|_| PANIC)?;
    let configured = scanner.options();
    if configured.ean_add_on_policy != add_on_policy {
        *scanner = Scanner::new(ScannerOptions {
            ean_add_on_policy: add_on_policy,
            ..configured
        });
    }
    let result = scanner
        .scan_with_options(
            image,
            ScanOptions {
                formats: Some(formats),
                debug: true,
                extended_budget: flags & 16 != 0,
            },
        )
        .map_err(|_| ARG)?;
    let mut raw = result.debug.as_ref().ok_or(PANIC)?.raw.clone();
    raw["elapsedMs"] = serde_json::json!(result.elapsed.as_secs_f64() * 1000.0);
    raw["multiple"] = serde_json::json!(flags & 1 == 0);
    if flags & 1 != 0 {
        let best = result.best();
        let index =
            best.and_then(|best| result.barcodes.iter().position(|b| std::ptr::eq(b, best)));
        let selected = index
            .and_then(|index| raw["scan"]["barcodes"].get(index).cloned())
            .into_iter()
            .collect::<Vec<_>>();
        raw["scan"]["barcodes"] = serde_json::json!(selected);
    }
    if flags & 2 == 0 {
        let scan = serde_json::json!({
            "unfinished": raw["scan"]["unfinished"],
            "barcodes": raw["scan"]["barcodes"],
        });
        raw = serde_json::json!({
            "schemaVersion": raw["schemaVersion"],
            "mode": raw["mode"],
            "multiple": raw["multiple"],
            "elapsedMs": raw["elapsedMs"],
            "localizationLimited": raw["localizationLimited"],
            "scan": scan,
        });
    }
    let barcodes = raw["scan"]["barcodes"].as_array().ok_or(PANIC)?;
    let mut reads = Vec::with_capacity(barcodes.len());
    let mut texts = Vec::with_capacity(barcodes.len());
    for b in barcodes {
        let mut read = BarcodeRead::default();
        for i in 0..4 {
            read.polygon[2 * i] = b["polygon"][i][0].as_f64().ok_or(PANIC)?;
            read.polygon[2 * i + 1] = b["polygon"][i][1].as_f64().ok_or(PANIC)?;
        }
        read.support = u32::try_from(b["support"].as_u64().ok_or(PANIC)?).map_err(|_| CAPACITY)?;
        let text = b["text"].as_str().ok_or(PANIC)?.as_bytes().to_vec();
        read.text_length = text.len() as u64;
        let format = b["format"].as_str().unwrap_or("EAN13").as_bytes();
        if format.len() >= read.format.len() {
            return Err(CAPACITY);
        }
        read.format[..format.len()].copy_from_slice(format);
        reads.push(read);
        texts.push(text);
    }
    Ok(Output {
        unfinished: raw["scan"]["unfinished"].as_bool().ok_or(PANIC)?,
        limited: raw["localizationLimited"].as_bool().ok_or(PANIC)?,
        json: serde_json::to_vec(&raw).map_err(|_| PANIC)?,
        reads,
        texts,
    })
}
/// Bit 0: finishing effort-selected EAN/UPC candidates is supported.
#[no_mangle]
pub extern "C" fn barcode_capabilities() -> u32 {
    1
}
#[no_mangle]
pub extern "C" fn barcode_abi_version() -> u32 {
    4
}
#[no_mangle]
pub extern "C" fn barcode_mode() -> u32 {
    MODE_ID
}
const MODE_ID: u32 = if cfg!(feature = "mode-low") {
    0
} else if cfg!(feature = "mode-medium") {
    1
} else if cfg!(feature = "mode-high") {
    2
} else {
    3
};
const MODE: Mode = if cfg!(feature = "mode-low") {
    Mode::Low
} else if cfg!(feature = "mode-medium") {
    Mode::Medium
} else if cfg!(feature = "mode-high") {
    Mode::High
} else {
    Mode::VeryHigh
};
/// # Safety
/// `out` must be null or point to writable, aligned storage for one `u64`.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_create(out: *mut u64) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(ARG);
        }
        *out = 0;
        let mut r = registry()?;
        if r.scanners.len() >= 1024 {
            return Err(CAPACITY);
        }
        let id = r.id()?;
        r.scanners.insert(
            id,
            Arc::new(Mutex::new(Scanner::new(ScannerOptions {
                mode: MODE,
                ..ScannerOptions::default()
            }))),
        );
        *out = id;
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn tapirscan_destroy(id: u64) -> i32 {
    boundary(|| registry()?.scanners.remove(&id).map(|_| ()).ok_or(HANDLE))
}
/// Input is borrowed only until this synchronous call returns. Errors clear out.
/// # Safety
/// `pixels` must be null or readable for `length` bytes for the duration of the call.
/// `out` must be null or writable and aligned for one `u64`, without overlapping pixels.
#[no_mangle]
pub unsafe extern "C" fn barcode_scan(
    id: u64,
    pixels: *const u8,
    length: u64,
    width: u64,
    height: u64,
    channels: u32,
    stride: u64,
    out: *mut u64,
) -> i32 {
    barcode_scan_with_options(id, pixels, length, width, height, channels, stride, 0, out)
}
/// Flags: 1 selects one read; 2 includes localized/search-region evidence.
/// # Safety
/// `pixels` must be null or readable for `length` bytes for the duration of the call.
/// `out` must be null or writable and aligned for one `u64`, without overlapping pixels.
#[no_mangle]
pub unsafe extern "C" fn barcode_scan_with_options(
    id: u64,
    pixels: *const u8,
    length: u64,
    width: u64,
    height: u64,
    channels: u32,
    stride: u64,
    flags: u32,
    out: *mut u64,
) -> i32 {
    barcode_scan_formats(
        id, pixels, length, width, height, channels, stride, flags, 15, out,
    )
}
/// Scan an explicit nonempty format bitmask; results own UTF-8 strings.
/// # Safety
/// Same pointer contract as `barcode_scan_with_options`.
#[no_mangle]
pub unsafe extern "C" fn barcode_scan_formats(
    id: u64,
    pixels: *const u8,
    length: u64,
    width: u64,
    height: u64,
    channels: u32,
    stride: u64,
    flags: u32,
    formats: u32,
    out: *mut u64,
) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(ARG);
        }
        *out = 0;
        if flags & !31 != 0
            || flags & 12 == 12
            || pixels.is_null()
            || length > MAX_BYTES
            || width < 3
            || height < 3
            || ![1, 3, 4].contains(&channels)
        {
            return Err(ARG);
        }
        let row = width.checked_mul(u64::from(channels)).ok_or(ARG)?;
        let required = (height - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(row))
            .ok_or(ARG)?;
        if stride < row || required > length || required > MAX_BYTES {
            return Err(ARG);
        }
        let formats = Formats::try_from(formats).map_err(|_| ARG)?;
        let scanner = registry()?.scanners.get(&id).cloned().ok_or(HANDLE)?;
        let data = std::slice::from_raw_parts(pixels, usize::try_from(required).map_err(|_| ARG)?);
        let width = usize::try_from(width).map_err(|_| ARG)?;
        let height = usize::try_from(height).map_err(|_| ARG)?;
        let image = match channels {
            1 => Image::gray(data, width, height),
            3 => Image::rgb(data, width, height),
            4 => Image::rgba(data, width, height),
            _ => return Err(ARG),
        }
        .with_stride(usize::try_from(stride).map_err(|_| ARG)?);
        let output = Arc::new(scan_output(&scanner, image, flags, formats)?);
        let mut r = registry()?;
        if r.results.len() >= 1024 {
            return Err(CAPACITY);
        }
        let result_id = r.id()?;
        r.results.insert(result_id, output);
        *out = result_id;
        Ok(())
    })
}
/// # Safety
/// `out` must be null or point to writable, aligned storage for one `ResultInfo`.
#[no_mangle]
pub unsafe extern "C" fn barcode_result_info(id: u64, out: *mut ResultInfo) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(ARG);
        }
        *out = ResultInfo::default();
        let r = output(id)?;
        *out = ResultInfo {
            json_length: r.json.len() as u64,
            barcode_count: r.reads.len() as u64,
            unfinished: u32::from(r.unfinished),
            localization_limited: u32::from(r.limited),
        };
        Ok(())
    })
}
/// # Safety
/// `out` must be null or point to writable, aligned storage for one `BarcodeRead`.
#[no_mangle]
pub unsafe extern "C" fn barcode_result_read(id: u64, index: u64, out: *mut BarcodeRead) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(ARG);
        }
        *out = BarcodeRead::default();
        let r = output(id)?;
        *out = *r
            .reads
            .get(usize::try_from(index).map_err(|_| ARG)?)
            .ok_or(ARG)?;
        Ok(())
    })
}
/// UTF-8 JSON plus a NUL terminator; required capacity is `json_length` + 1.
/// # Safety
/// `bytes` must be null or point to writable storage of at least `capacity` bytes.
#[no_mangle]
pub unsafe extern "C" fn barcode_result_copy_json(id: u64, bytes: *mut u8, capacity: u64) -> i32 {
    boundary(|| {
        if bytes.is_null() {
            return Err(ARG);
        }
        let r = output(id)?;
        if capacity <= r.json.len() as u64 {
            return Err(BUFFER);
        }
        std::ptr::copy_nonoverlapping(r.json.as_ptr(), bytes, r.json.len());
        *bytes.add(r.json.len()) = 0;
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn barcode_result_destroy(id: u64) -> i32 {
    boundary(|| registry()?.results.remove(&id).map(|_| ()).ok_or(HANDLE))
}
/// Copy the full UTF-8 payload plus NUL; embedded NUL bytes are preserved.
/// # Safety
/// `bytes` must be null or writable for `capacity` bytes, without overlap.
#[no_mangle]
pub unsafe extern "C" fn barcode_result_copy_text(
    id: u64,
    index: u64,
    bytes: *mut u8,
    capacity: u64,
) -> i32 {
    boundary(|| {
        if bytes.is_null() {
            return Err(ARG);
        }
        let result = output(id)?;
        let text = result
            .texts
            .get(usize::try_from(index).map_err(|_| ARG)?)
            .ok_or(ARG)?;
        if capacity <= text.len() as u64 {
            return Err(BUFFER);
        }
        std::ptr::copy_nonoverlapping(text.as_ptr(), bytes, text.len());
        *bytes.add(text.len()) = 0;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supplement_flags_are_mutually_exclusive() {
        unsafe {
            let mut id = 0;
            assert_eq!(tapirscan_create(&raw mut id), 0);
            let pixels = [255u8; 64 * 64];
            for flags in [4, 8, 12, 16, 20, 24, 32] {
                let mut result = 99;
                let status = barcode_scan_with_options(
                    id,
                    pixels.as_ptr(),
                    pixels.len() as u64,
                    64,
                    64,
                    1,
                    64,
                    flags,
                    &raw mut result,
                );
                if matches!(flags, 4 | 8 | 16 | 20 | 24) {
                    assert_eq!(status, 0);
                    assert_eq!(barcode_result_destroy(result), 0);
                } else {
                    assert_eq!(status, ARG);
                    assert_eq!(result, 0);
                }
            }
            assert_eq!(tapirscan_destroy(id), 0);
        }
    }

    #[test]
    fn handles_buffers_and_owned_results() {
        unsafe {
            assert_eq!(std::mem::size_of::<BarcodeRead>(), 104);
            assert_eq!(std::mem::size_of::<ResultInfo>(), 24);
            assert_eq!(tapirscan_create(std::ptr::null_mut()), ARG);
            let mut id = 0;
            assert_eq!(tapirscan_create(&raw mut id), 0);
            let pixels = vec![255u8; 64 * 64];
            let mut result = 99;
            assert_eq!(
                barcode_scan(id, pixels.as_ptr(), 1, 64, 64, 1, 64, &raw mut result),
                ARG
            );
            assert_eq!(result, 0);
            assert_eq!(
                barcode_scan_with_options(
                    id,
                    pixels.as_ptr(),
                    pixels.len() as u64,
                    64,
                    64,
                    1,
                    64,
                    32,
                    &raw mut result
                ),
                ARG
            );
            assert_eq!(result, 0);
            assert_eq!(
                barcode_scan(
                    id,
                    pixels.as_ptr(),
                    pixels.len() as u64,
                    64,
                    64,
                    1,
                    64,
                    &raw mut result
                ),
                0
            );
            assert_eq!(tapirscan_destroy(id), 0);
            assert_eq!(tapirscan_destroy(id), HANDLE);
            let mut info = ResultInfo::default();
            assert_eq!(barcode_result_info(result, &raw mut info), 0);
            let mut bytes = vec![0; usize::try_from(info.json_length).unwrap() + 1];
            assert_eq!(
                barcode_result_copy_json(result, bytes.as_mut_ptr(), 1),
                BUFFER
            );
            assert_eq!(
                barcode_result_copy_json(result, bytes.as_mut_ptr(), bytes.len() as u64),
                0
            );
            assert_eq!(bytes.last(), Some(&0));
            let json =
                std::str::from_utf8(&bytes[..usize::try_from(info.json_length).unwrap()]).unwrap();
            assert!(!json.contains("\"candidates\""));
            assert!(!json.contains("\"searchWindows\""));
            assert_eq!(barcode_result_destroy(result), 0);
            assert_eq!(barcode_result_info(result, &raw mut info), HANDLE);
            assert_eq!(info.json_length, 0);
        }
    }
    #[test]
    fn catches_unwinding() {
        assert_eq!(boundary(|| panic!("boundary probe")), PANIC);
    }
}
