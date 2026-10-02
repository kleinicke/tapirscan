//! Native ABI 5. Handles are registry IDs, never dereferenced pointers.
//! Caller-owned pointer ranges must be valid, correctly aligned and nonoverlapping.
#[cfg(not(target_pointer_width = "64"))]
compile_error!("Native ABI 5 currently supports 64-bit targets only");
use std::{
    collections::HashMap,
    ffi::{c_char, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex, OnceLock},
};
use tapirscan_api::{
    Barcode, EanAddOnPolicy, Format, Formats, Image, Mode, ScanOptions, ScanResult, Scanner,
    ScannerOptions, UndecodedRegion,
};

const ABI_VERSION: u32 = 5;
const ARG: i32 = 1;
const HANDLE: i32 = 2;
const BUFFER: i32 = 3;
const PANIC: i32 = 4;
const CAPACITY: i32 = 5;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_HANDLES: usize = 1024;
/// Length of an optional field that the reader did not report.
const ABSENT: u64 = u64::MAX;

const FIELD_TEXT: u32 = 0;
const FIELD_PAYLOAD_BYTES: u32 = 1;
const FIELD_EAN_ADD_ON: u32 = 2;
const FIELD_STRUCTURED_APPEND_ID: u32 = 3;

#[repr(C)]
pub struct ScannerOptionsC {
    pub mode: u32,
    pub formats: u32,
    pub ean_add_on_policy: u32,
}
#[repr(C)]
pub struct ImageC {
    pub data: *const u8,
    pub length: u64,
    pub width: u64,
    pub height: u64,
    pub channels: u32,
    pub stride: u64,
}
#[repr(C)]
pub struct ScanOptionsC {
    pub formats: u32,
    pub debug: u32,
    pub extended_budget: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PointC {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ResultInfoC {
    pub barcode_count: u64,
    pub undecoded_count: u64,
    pub best_index: i64,
    pub json_length: u64,
    pub width: u64,
    pub height: u64,
    pub elapsed_ms: f64,
    pub mode: u32,
    pub unfinished: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BarcodeC {
    pub polygon: [PointC; 4],
    pub support: u64,
    pub format: u32,
    pub gs1: i32,
    pub reader_initialization: i32,
    pub structured_append_parity: i32,
    pub structured_append_index: u64,
    pub structured_append_count: u64,
    pub text_length: u64,
    pub payload_bytes_length: u64,
    pub ean_add_on_length: u64,
    pub structured_append_id_length: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RegionC {
    pub polygon: [PointC; 4],
    pub format: u32,
}

struct Output {
    result: ScanResult,
    json: Vec<u8>,
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
fn index(value: u64) -> Result<usize, i32> {
    usize::try_from(value).map_err(|_| ARG)
}
fn polygon(points: &[[f64; 2]; 4]) -> [PointC; 4] {
    points.map(|[x, y]| PointC { x, y })
}
fn tristate(value: Option<bool>) -> i32 {
    value.map_or(-1, i32::from)
}
fn length(value: Option<usize>) -> u64 {
    value.map_or(ABSENT, |n| n as u64)
}
fn mode_from(value: u32) -> Result<Mode, i32> {
    match value {
        0 => Ok(Mode::Low),
        1 => Ok(Mode::Medium),
        2 => Ok(Mode::High),
        3 => Ok(Mode::VeryHigh),
        _ => Err(ARG),
    }
}
fn mode_id(mode: Mode) -> u32 {
    match mode {
        Mode::Low => 0,
        Mode::Medium => 1,
        Mode::High => 2,
        Mode::VeryHigh => 3,
    }
}

/// Serialize schema 2: the complete engine evidence for debug scans, otherwise
/// the compact decoded result.
fn result_json(result: &ScanResult, debug: bool) -> Result<Vec<u8>, i32> {
    let mut raw = result.debug.as_ref().ok_or(PANIC)?.raw.clone();
    raw["elapsedMs"] = serde_json::json!(result.elapsed.as_secs_f64() * 1000.0);
    raw["multiple"] = serde_json::json!(true);
    if !debug {
        raw = serde_json::json!({
            "schemaVersion": raw["schemaVersion"],
            "mode": raw["mode"],
            "multiple": raw["multiple"],
            "elapsedMs": raw["elapsedMs"],
            "localizationLimited": raw["localizationLimited"],
            "scan": {
                "unfinished": raw["scan"]["unfinished"],
                "barcodes": raw["scan"]["barcodes"],
            },
        });
    }
    serde_json::to_vec(&raw).map_err(|_| PANIC)
}

#[no_mangle]
pub extern "C" fn tapirscan_abi_version() -> u32 {
    ABI_VERSION
}

/// Static, NUL-terminated description of a status code.
#[no_mangle]
pub extern "C" fn tapirscan_status_message(status: i32) -> *const c_char {
    let message: &'static [u8] = match status {
        0 => b"Success\0",
        ARG => b"Invalid scanner arguments or image parameters\0",
        HANDLE => b"Invalid scanner or result handle; it may already have been destroyed\0",
        BUFFER => b"Result buffer is too small\0",
        PANIC => b"Internal scanner failure\0",
        CAPACITY => b"Scanner resource capacity exceeded; destroy unused scanners and results\0",
        _ => b"Unknown scanner status\0",
    };
    message.as_ptr().cast()
}

/// Static, NUL-terminated name of one format bit, or null for anything else.
#[no_mangle]
pub extern "C" fn tapirscan_format_name(format: u32) -> *const c_char {
    static NAMES: OnceLock<Vec<(u32, CString)>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            Format::ALL
                .iter()
                .filter_map(|f| Some((*f as u32, CString::new(f.as_str()).ok()?)))
                .collect()
        })
        .iter()
        .find(|(bits, _)| *bits == format)
        .map_or(std::ptr::null(), |(_, name)| name.as_ptr())
}

/// Null options select Medium, Retail formats and ignored EAN/UPC supplements.
/// # Safety
/// `options` must be null or readable; `out` must be null or writable for one `u64`.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_scanner_create(
    options: *const ScannerOptionsC,
    out: *mut u64,
) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(ARG);
        }
        *out = 0;
        let options = match options.as_ref() {
            None => ScannerOptions::default(),
            Some(options) => ScannerOptions {
                mode: mode_from(options.mode)?,
                formats: Formats::try_from(options.formats).map_err(|_| ARG)?,
                ean_add_on_policy: match options.ean_add_on_policy {
                    0 => EanAddOnPolicy::Ignore,
                    1 => EanAddOnPolicy::Read,
                    2 => EanAddOnPolicy::Require,
                    _ => return Err(ARG),
                },
            },
        };
        let mut r = registry()?;
        if r.scanners.len() >= MAX_HANDLES {
            return Err(CAPACITY);
        }
        let id = r.id()?;
        r.scanners
            .insert(id, Arc::new(Mutex::new(Scanner::new(options))));
        *out = id;
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn tapirscan_scanner_destroy(scanner: u64) -> i32 {
    boundary(|| {
        registry()?
            .scanners
            .remove(&scanner)
            .map(|_| ())
            .ok_or(HANDLE)
    })
}

/// Pixels are borrowed only until this synchronous call returns. Null scan
/// options, and zero fields within them, select the scanner defaults.
/// # Safety
/// `image` must be readable and its `data` readable for `length` bytes.
/// `options` must be null or readable; `out` must be null or writable for one `u64`.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_scan(
    scanner: u64,
    image: *const ImageC,
    options: *const ScanOptionsC,
    out: *mut u64,
) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(ARG);
        }
        *out = 0;
        let image = image.as_ref().ok_or(ARG)?;
        let (formats, debug, extended_budget) = match options.as_ref() {
            None => (0, false, false),
            Some(o) if o.debug > 1 || o.extended_budget > 1 => return Err(ARG),
            Some(o) => (o.formats, o.debug == 1, o.extended_budget == 1),
        };
        let formats = match formats {
            0 => None,
            bits => Some(Formats::try_from(bits).map_err(|_| ARG)?),
        };
        if image.data.is_null()
            || image.length > MAX_BYTES
            || image.width < 3
            || image.height < 3
            || ![1, 3, 4].contains(&image.channels)
        {
            return Err(ARG);
        }
        let row = image
            .width
            .checked_mul(u64::from(image.channels))
            .ok_or(ARG)?;
        let stride = if image.stride == 0 { row } else { image.stride };
        let required = (image.height - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(row))
            .ok_or(ARG)?;
        if stride < row || required > image.length || required > MAX_BYTES {
            return Err(ARG);
        }
        let scanner = registry()?.scanners.get(&scanner).cloned().ok_or(HANDLE)?;
        let data = std::slice::from_raw_parts(image.data, index(required)?);
        let (width, height) = (index(image.width)?, index(image.height)?);
        let pixels = match image.channels {
            1 => Image::gray(data, width, height),
            3 => Image::rgb(data, width, height),
            _ => Image::rgba(data, width, height),
        }
        .with_stride(index(stride)?);
        // Diagnostics are always retained: the compact JSON is derived from them.
        let result = scanner
            .lock()
            .map_err(|_| PANIC)?
            .scan_with_options(
                pixels,
                ScanOptions {
                    formats,
                    debug: true,
                    extended_budget,
                },
            )
            .map_err(|error| match error {
                tapirscan_api::Error::Engine(_) => PANIC,
                _ => ARG,
            })?;
        let json = result_json(&result, debug)?;
        let mut result = result;
        if !debug {
            result.debug = None;
        }
        let mut r = registry()?;
        if r.results.len() >= MAX_HANDLES {
            return Err(CAPACITY);
        }
        let id = r.id()?;
        r.results.insert(id, Arc::new(Output { result, json }));
        *out = id;
        Ok(())
    })
}

/// # Safety
/// `out` must be null or writable and aligned for one `tapirscan_summary`.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_result_info(result: u64, out: *mut ResultInfoC) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(ARG);
        }
        *out = ResultInfoC::default();
        let output = output(result)?;
        let r = &output.result;
        let best = r
            .best()
            .and_then(|best| r.barcodes.iter().position(|b| std::ptr::eq(b, best)));
        *out = ResultInfoC {
            barcode_count: r.barcodes.len() as u64,
            undecoded_count: r.undecoded.len() as u64,
            best_index: best.map_or(-1, |i| i64::try_from(i).unwrap_or(-1)),
            json_length: output.json.len() as u64,
            width: r.image_size[0] as u64,
            height: r.image_size[1] as u64,
            elapsed_ms: r.elapsed.as_secs_f64() * 1000.0,
            mode: mode_id(r.mode),
            unfinished: u32::from(r.unfinished),
        };
        Ok(())
    })
}

fn barcode_c(b: &Barcode) -> BarcodeC {
    let append = b.structured_append.as_ref();
    BarcodeC {
        polygon: polygon(&b.polygon),
        support: b.support,
        format: b.format as u32,
        gs1: tristate(b.gs1),
        reader_initialization: tristate(b.reader_initialization),
        structured_append_parity: append.and_then(|a| a.parity).map_or(-1, i32::from),
        structured_append_index: append.map_or(0, |a| a.index as u64),
        structured_append_count: append.map_or(0, |a| a.count as u64),
        text_length: b.text.len() as u64,
        payload_bytes_length: length(b.payload_bytes.as_ref().map(Vec::len)),
        ean_add_on_length: length(b.ean_add_on.as_ref().map(String::len)),
        structured_append_id_length: length(append.and_then(|a| a.id.as_ref()).map(String::len)),
    }
}

/// # Safety
/// `out` must be null or writable and aligned for one `tapirscan_barcode`.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_result_barcode(
    result: u64,
    position: u64,
    out: *mut BarcodeC,
) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(ARG);
        }
        *out = BarcodeC::default();
        let output = output(result)?;
        let barcode = output.result.barcodes.get(index(position)?).ok_or(ARG)?;
        *out = barcode_c(barcode);
        Ok(())
    })
}

/// # Safety
/// `out` must be null or writable and aligned for one `tapirscan_region`.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_result_undecoded(
    result: u64,
    position: u64,
    out: *mut RegionC,
) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(ARG);
        }
        *out = RegionC::default();
        let output = output(result)?;
        let region: &UndecodedRegion = output.result.undecoded.get(index(position)?).ok_or(ARG)?;
        *out = RegionC {
            polygon: polygon(&region.polygon),
            format: region.format.map_or(0, |f| f as u32),
        };
        Ok(())
    })
}

unsafe fn copy_bytes(value: &[u8], out: *mut u8, capacity: u64) -> Result<(), i32> {
    if out.is_null() {
        return Err(ARG);
    }
    if capacity <= value.len() as u64 {
        return Err(BUFFER);
    }
    std::ptr::copy_nonoverlapping(value.as_ptr(), out, value.len());
    *out.add(value.len()) = 0;
    Ok(())
}

/// Copy one variable-length barcode field plus a NUL terminator. Embedded NUL
/// bytes are preserved; use the reported length. Absent fields are rejected.
/// # Safety
/// `out` must be null or writable for `capacity` bytes, without overlap.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_result_copy(
    result: u64,
    position: u64,
    field: u32,
    out: *mut u8,
    capacity: u64,
) -> i32 {
    boundary(|| {
        let output = output(result)?;
        let b = output.result.barcodes.get(index(position)?).ok_or(ARG)?;
        let value: &[u8] = match field {
            FIELD_TEXT => b.text.as_bytes(),
            FIELD_PAYLOAD_BYTES => b.payload_bytes.as_deref().ok_or(ARG)?,
            FIELD_EAN_ADD_ON => b.ean_add_on.as_deref().ok_or(ARG)?.as_bytes(),
            FIELD_STRUCTURED_APPEND_ID => b
                .structured_append
                .as_ref()
                .and_then(|a| a.id.as_deref())
                .ok_or(ARG)?
                .as_bytes(),
            _ => return Err(ARG),
        };
        copy_bytes(value, out, capacity)
    })
}

/// UTF-8 schema-2 JSON plus a NUL terminator; capacity must exceed `json_length`.
/// # Safety
/// `out` must be null or writable for `capacity` bytes.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_result_copy_json(
    result: u64,
    out: *mut u8,
    capacity: u64,
) -> i32 {
    boundary(|| copy_bytes(&output(result)?.json, out, capacity))
}

#[no_mangle]
pub extern "C" fn tapirscan_result_destroy(result: u64) -> i32 {
    boundary(|| {
        registry()?
            .results
            .remove(&result)
            .map(|_| ())
            .ok_or(HANDLE)
    })
}

#[cfg(test)]
mod tests;
