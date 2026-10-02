//! Native ABI 6. Handles are registry IDs, never dereferenced pointers.
//! Caller-owned pointer ranges must be valid, correctly aligned and nonoverlapping.
#[cfg(not(target_pointer_width = "64"))]
compile_error!("Native ABI 6 currently supports 64-bit targets only");
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

const ABI_VERSION: u32 = 6;
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

/// Caller-owned, NUL-terminated UTF-8 diagnostic; long messages are truncated.
#[repr(C)]
pub struct ErrorC {
    pub message: [u8; 512],
}

struct Failure {
    code: i32,
    message: String,
}
impl From<i32> for Failure {
    fn from(code: i32) -> Self {
        Self {
            code,
            message: status_text(code).to_string_lossy().into_owned(),
        }
    }
}
impl From<tapirscan_api::Error> for Failure {
    fn from(error: tapirscan_api::Error) -> Self {
        let code = if matches!(error, tapirscan_api::Error::Engine(_)) {
            PANIC
        } else {
            ARG
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}
fn invalid(message: &str) -> Failure {
    Failure {
        code: ARG,
        message: message.into(),
    }
}
unsafe fn detailed(error: *mut ErrorC, f: impl FnOnce() -> Result<(), Failure>) -> i32 {
    if let Some(error) = error.as_mut() {
        error.message.fill(0);
    }
    let failure = match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => return 0,
        Ok(Err(failure)) => failure,
        Err(_) => Failure::from(PANIC),
    };
    if let Some(error) = error.as_mut() {
        let mut length = failure.message.len().min(error.message.len() - 1);
        while !failure.message.is_char_boundary(length) {
            length -= 1;
        }
        error.message[..length].copy_from_slice(&failure.message.as_bytes()[..length]);
    }
    failure.code
}
fn status_text(code: i32) -> &'static std::ffi::CStr {
    match code {
        0 => c"Success",
        ARG => c"Invalid scanner arguments or image parameters",
        HANDLE => c"Invalid scanner or result handle; it may already have been destroyed",
        BUFFER => c"Result buffer is too small",
        PANIC => c"Internal scanner failure",
        CAPACITY => c"Scanner resource capacity exceeded; destroy unused scanners and results",
        _ => c"Unknown scanner status",
    }
}

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
    barcodes: Vec<Barcode>,
    report: Option<ScanResult>,
    json: OnceLock<Vec<u8>>,
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

/// JSON is built only for JSON consumers; typed access never serializes.
impl Output {
    fn json(&self) -> &[u8] {
        self.json.get_or_init(|| {
            let raw = if let Some(report) = &self.report {
                let mut raw = report
                    .diagnostics
                    .as_ref()
                    .expect("inspection diagnostics")
                    .raw
                    .clone();
                raw["elapsedMs"] = serde_json::json!(report.elapsed.as_secs_f64() * 1000.0);
                raw["multiple"] = serde_json::json!(true);
                raw
            } else {
                let barcodes: Vec<_> = self
                    .barcodes
                    .iter()
                    .map(|b| {
                        let mut value = serde_json::json!({
                            "text": b.text, "format": b.format.as_str(),
                            "polygon": b.polygon, "support": b.support,
                        });
                        if let Some(bytes) = &b.payload_bytes {
                            value["bytes"] = serde_json::json!(bytes);
                        }
                        if let Some(addon) = &b.ean_add_on {
                            value["eanAddOn"] = serde_json::json!(addon);
                        }
                        if let Some(gs1) = b.gs1 {
                            value["gs1"] = serde_json::json!(gs1);
                        }
                        if let Some(init) = b.reader_initialization {
                            value["readerInitialization"] = serde_json::json!(init);
                        }
                        if let Some(append) = &b.structured_append {
                            value["structuredAppend"] = serde_json::json!({
                                "index": append.index, "count": append.count,
                                "id": append.id, "parity": append.parity,
                            });
                        }
                        value
                    })
                    .collect();
                serde_json::json!(barcodes)
            };
            // JSON Values contain no non-finite floats or fallible custom serializers.
            serde_json::to_vec(&raw).expect("JSON value serialization")
        })
    }
}

/// Native ABI version expected by this library.
#[no_mangle]
pub extern "C" fn tapirscan_abi_version() -> u32 {
    ABI_VERSION
}

/// Static, NUL-terminated description of a status code.
#[no_mangle]
pub extern "C" fn tapirscan_status_message(status: i32) -> *const c_char {
    status_text(status).as_ptr()
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
/// `error` must be null or writable for one `ErrorC`, disjoint from other arguments.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_scanner_create(
    options: *const ScannerOptionsC,
    out: *mut u64,
    error: *mut ErrorC,
) -> i32 {
    detailed(error, || {
        if out.is_null() {
            return Err(invalid("output handle pointer is null"));
        }
        *out = 0;
        let options = match options.as_ref() {
            None => ScannerOptions::default(),
            Some(options) => ScannerOptions {
                mode: mode_from(options.mode).map_err(|_| invalid("unknown scanner mode"))?,
                formats: Formats::try_from(options.formats).map_err(Failure::from)?,
                ean_add_on_policy: match options.ean_add_on_policy {
                    0 => EanAddOnPolicy::Ignore,
                    1 => EanAddOnPolicy::Read,
                    2 => EanAddOnPolicy::Require,
                    _ => return Err(invalid("unknown EAN add-on policy")),
                },
            },
        };
        let mut r = registry()?;
        if r.scanners.len() >= MAX_HANDLES {
            return Err(CAPACITY.into());
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
/// `error` must be null or writable for one `ErrorC`, disjoint from other arguments.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_scan(
    scanner: u64,
    image: *const ImageC,
    options: *const ScanOptionsC,
    out: *mut u64,
    error: *mut ErrorC,
) -> i32 {
    scan(scanner, image, options, out, error, false)
}

/// Inspect an image, retaining work status and diagnostic evidence.
/// # Safety
/// Same pointer requirements as [`tapirscan_scan`].
#[no_mangle]
pub unsafe extern "C" fn tapirscan_inspect(
    scanner: u64,
    image: *const ImageC,
    options: *const ScanOptionsC,
    out: *mut u64,
    error: *mut ErrorC,
) -> i32 {
    scan(scanner, image, options, out, error, true)
}

unsafe fn scan(
    scanner: u64,
    image: *const ImageC,
    options: *const ScanOptionsC,
    out: *mut u64,
    error: *mut ErrorC,
    inspect: bool,
) -> i32 {
    detailed(error, || {
        if out.is_null() {
            return Err(invalid("output handle pointer is null"));
        }
        *out = 0;
        let image = image
            .as_ref()
            .ok_or_else(|| invalid("image pointer is null"))?;
        let (formats, extended_budget) = match options.as_ref() {
            None => (0, false),
            Some(o) if o.extended_budget > 1 => {
                return Err(invalid("extended_budget must be 0 or 1"))
            }
            Some(o) => (o.formats, o.extended_budget == 1),
        };
        let formats = match formats {
            0 => None,
            bits => Some(Formats::try_from(bits).map_err(Failure::from)?),
        };
        if image.data.is_null() {
            return Err(invalid("pixel pointer is null"));
        }
        if image.length > MAX_BYTES {
            return Err(invalid("pixel buffer exceeds 128 MiB"));
        }
        if image.width < 3 || image.height < 3 {
            return Err(invalid("dimensions must be at least 3x3"));
        }
        if ![1, 3, 4].contains(&image.channels) {
            return Err(invalid("channels must be 1, 3 or 4"));
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
        if stride < row {
            return Err(invalid("stride is smaller than a pixel row"));
        }
        if required > image.length || required > MAX_BYTES {
            return Err(invalid(
                "buffer is too short or exceeds the 128 MiB layout limit",
            ));
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
        let mut scanner = scanner.lock().map_err(|_| PANIC)?;
        let options = ScanOptions {
            formats,
            extended_budget,
        };
        let (barcodes, report) = if inspect {
            let mut report = scanner
                .inspect_with_options(pixels, options)
                .map_err(Failure::from)?;
            (std::mem::take(&mut report.barcodes), Some(report))
        } else {
            (
                scanner
                    .scan_with_options(pixels, options)
                    .map_err(Failure::from)?,
                None,
            )
        };
        let mut r = registry()?;
        if r.results.len() >= MAX_HANDLES {
            return Err(CAPACITY.into());
        }
        let id = r.id()?;
        r.results.insert(
            id,
            Arc::new(Output {
                barcodes,
                report,
                json: OnceLock::new(),
            }),
        );
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
        let r = output.report.as_ref().ok_or(ARG)?;
        let best = output
            .barcodes
            .iter()
            .enumerate()
            .max_by_key(|(i, b)| (b.support, std::cmp::Reverse(*i)))
            .map(|(i, _)| i);
        *out = ResultInfoC {
            barcode_count: output.barcodes.len() as u64,
            undecoded_count: r.undecoded.len() as u64,
            best_index: best.map_or(-1, |i| i64::try_from(i).unwrap_or(-1)),
            width: r.image_size[0] as u64,
            height: r.image_size[1] as u64,
            elapsed_ms: r.elapsed.as_secs_f64() * 1000.0,
            mode: mode_id(r.mode),
            unfinished: u32::from(r.unfinished),
        };
        Ok(())
    })
}

/// Return the number of decoded barcodes for either operation.
/// # Safety
/// `out` must be null or writable and aligned for one u64.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_result_count(result: u64, out: *mut u64) -> i32 {
    boundary(|| {
        *out.as_mut().ok_or(ARG)? = output(result)?.barcodes.len() as u64;
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
        let barcode = output.barcodes.get(index(position)?).ok_or(ARG)?;
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
        let region: &UndecodedRegion = output
            .report
            .as_ref()
            .ok_or(ARG)?
            .undecoded
            .get(index(position)?)
            .ok_or(ARG)?;
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
        let b = output.barcodes.get(index(position)?).ok_or(ARG)?;
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

/// Return the JSON byte length (excluding NUL), serializing once on demand.
/// # Safety
/// `out` must be null or writable and aligned for one u64.
#[no_mangle]
pub unsafe extern "C" fn tapirscan_result_json_length(result: u64, out: *mut u64) -> i32 {
    boundary(|| {
        let out = out.as_mut().ok_or(ARG)?;
        *out = output(result)?.json().len() as u64;
        Ok(())
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
    boundary(|| copy_bytes(output(result)?.json(), out, capacity))
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
