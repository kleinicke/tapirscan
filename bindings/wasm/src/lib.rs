//! Import-free raw WASM ABI over the public Tapirscan Rust API.

use std::{cell::RefCell, collections::HashMap};
use tapirscan::{
    Barcode, EanAddOnPolicy, Formats, Image, Mode, ScanOptions, ScanResult, Scanner,
    ScannerOptions, StructuredAppend,
};

const OK: i32 = 0;
const ARG: i32 = 1;
const HANDLE: i32 = 2;
const INTERNAL: i32 = 4;
const CAPACITY: i32 = 5;
const MAX_BYTES: usize = 128 * 1024 * 1024;
const MAX_SCANNERS: usize = 1_024;
const ABI_VERSION: u32 = 1;

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
#[derive(Clone, Copy)]
struct ImageSpec {
    width: usize,
    height: usize,
    channels: usize,
    stride: usize,
}

struct Session {
    scanner: Scanner,
    input: Vec<u8>,
    output: Vec<u8>,
    image: Option<ImageSpec>,
}

#[derive(Default)]
struct Registry {
    next: u32,
    sessions: HashMap<u32, Session>,
}

impl Registry {
    fn id(&mut self) -> Result<u32, i32> {
        self.next = self.next.checked_add(1).ok_or(CAPACITY)?;
        if self.next == 0 {
            return Err(CAPACITY);
        }
        Ok(self.next)
    }
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

fn add_on_policy(value: u32) -> Result<EanAddOnPolicy, i32> {
    match value {
        0 => Ok(EanAddOnPolicy::Ignore),
        1 => Ok(EanAddOnPolicy::Read),
        2 => Ok(EanAddOnPolicy::Require),
        _ => Err(ARG),
    }
}

fn pointer(bytes: &[u8]) -> u32 {
    u32::try_from(bytes.as_ptr() as usize).unwrap_or(0)
}

fn mutable_pointer(bytes: &mut [u8]) -> u32 {
    u32::try_from(bytes.as_mut_ptr() as usize).unwrap_or(0)
}

/// Raw WASM adapter ABI version.
#[no_mangle]
pub extern "C" fn tapirscan_abi_version() -> u32 {
    ABI_VERSION
}

/// Compiled effort mode: low=0, medium=1, high=2, very-high=3.
#[no_mangle]
pub extern "C" fn tapirscan_mode() -> u32 {
    MODE_ID
}

/// Create a scanner. Returns zero for invalid configuration or capacity exhaustion.
#[no_mangle]
pub extern "C" fn tapirscan_create(mode: u32, format_mask: u32, addon_policy: u32) -> u32 {
    if mode != MODE_ID {
        return 0;
    }
    let Ok(formats) = Formats::try_from(format_mask) else {
        return 0;
    };
    let Ok(ean_add_on_policy) = add_on_policy(addon_policy) else {
        return 0;
    };
    REGISTRY.with_borrow_mut(|registry| {
        if registry.sessions.len() >= MAX_SCANNERS {
            return 0;
        }
        let Ok(id) = registry.id() else {
            return 0;
        };
        registry.sessions.insert(
            id,
            Session {
                scanner: Scanner::new(ScannerOptions {
                    mode: MODE,
                    formats,
                    ean_add_on_policy,
                }),
                input: Vec::new(),
                output: Vec::new(),
                image: None,
            },
        );
        id
    })
}

/// Destroy a scanner and invalidate its input and output views.
#[no_mangle]
pub extern "C" fn tapirscan_destroy(handle: u32) -> i32 {
    REGISTRY.with_borrow_mut(|registry| {
        if registry.sessions.remove(&handle).is_some() {
            OK
        } else {
            HANDLE
        }
    })
}

/// Allocate the validated pixel buffer. Its pointer is stable until the next prepare.
#[no_mangle]
pub extern "C" fn tapirscan_prepare(
    handle: u32,
    width: u32,
    height: u32,
    channels: u32,
    stride: u32,
) -> i32 {
    if width < 3 || height < 3 || !matches!(channels, 1 | 3 | 4) {
        return ARG;
    }
    let Ok(row) = width.checked_mul(channels).ok_or(ARG) else {
        return ARG;
    };
    if stride < row {
        return ARG;
    }
    let Ok(length) = (height - 1)
        .checked_mul(stride)
        .and_then(|value| value.checked_add(row))
        .ok_or(ARG)
        .and_then(|value| usize::try_from(value).map_err(|_| ARG))
    else {
        return ARG;
    };
    if length > MAX_BYTES {
        return ARG;
    }
    REGISTRY.with_borrow_mut(|registry| {
        let Some(session) = registry.sessions.get_mut(&handle) else {
            return HANDLE;
        };
        session.input.resize(length, 0);
        session.output.clear();
        session.image = Some(ImageSpec {
            width: width as usize,
            height: height as usize,
            channels: channels as usize,
            stride: stride as usize,
        });
        OK
    })
}

/// Pointer to the prepared pixel buffer, or zero for an invalid/unprepared handle.
#[no_mangle]
pub extern "C" fn tapirscan_input_ptr(handle: u32) -> u32 {
    REGISTRY.with_borrow_mut(|registry| {
        registry
            .sessions
            .get_mut(&handle)
            .filter(|session| session.image.is_some())
            .map_or(0, |session| mutable_pointer(&mut session.input))
    })
}

/// Exact writable byte length returned by prepare.
#[no_mangle]
pub extern "C" fn tapirscan_input_len(handle: u32) -> u32 {
    REGISTRY.with_borrow(|registry| {
        registry
            .sessions
            .get(&handle)
            .filter(|session| session.image.is_some())
            .and_then(|session| u32::try_from(session.input.len()).ok())
            .unwrap_or(0)
    })
}

/// Scan flags: bit 0 extended budget, bit 1 raw diagnostics. Zero format mask uses
/// the scanner's configured selection; a nonzero mask overrides it for this call.
#[no_mangle]
pub extern "C" fn tapirscan_scan(handle: u32, flags: u32, format_mask: u32) -> i32 {
    if flags & !3 != 0 {
        return ARG;
    }
    let formats = if format_mask == 0 {
        None
    } else {
        match Formats::try_from(format_mask) {
            Ok(formats) => Some(formats),
            Err(_) => return ARG,
        }
    };
    REGISTRY.with_borrow_mut(|registry| {
        let Some(session) = registry.sessions.get_mut(&handle) else {
            return HANDLE;
        };
        let Some(spec) = session.image else {
            return ARG;
        };
        session.output.clear();
        let image = match spec.channels {
            1 => Image::gray(&session.input, spec.width, spec.height),
            3 => Image::rgb(&session.input, spec.width, spec.height),
            4 => Image::rgba(&session.input, spec.width, spec.height),
            _ => return ARG,
        }
        .with_stride(spec.stride);
        let result = session.scanner.scan_with_options(
            image,
            ScanOptions {
                formats,
                debug: flags & 2 != 0,
                extended_budget: flags & 1 != 0,
            },
        );
        match result {
            Ok(result) => match serde_json::to_vec(&wire_result(&result)) {
                Ok(output) => {
                    session.output = output;
                    OK
                }
                Err(error) => store_error(session, &error.to_string()),
            },
            Err(error) => store_error(session, &error.to_string()),
        }
    })
}

fn store_error(session: &mut Session, message: &str) -> i32 {
    session.output = serde_json::to_vec(&serde_json::json!({ "error": message }))
        .unwrap_or_else(|_| br#"{"error":"scanner failed"}"#.to_vec());
    INTERNAL
}

fn wire_result(result: &ScanResult) -> serde_json::Value {
    let best_index = result.best().and_then(|best| {
        result
            .barcodes
            .iter()
            .position(|barcode| std::ptr::eq(barcode, best))
    });
    let barcodes = result.barcodes.iter().map(wire_barcode).collect::<Vec<_>>();
    let undecoded = result
        .undecoded
        .iter()
        .map(|region| {
            serde_json::json!({
                "format": region.format.map_or("Unknown", tapirscan::Format::as_str),
                "polygon": region.polygon,
            })
        })
        .collect::<Vec<_>>();
    let mut wire = serde_json::json!({
        "barcodes": barcodes,
        "bestIndex": best_index,
        "undecoded": undecoded,
        "image": { "width": result.image_size[0], "height": result.image_size[1] },
        "mode": result.mode.as_str(),
        "elapsedMs": result.elapsed.as_secs_f64() * 1000.0,
        "unfinished": result.unfinished,
    });
    if let Some(debug) = &result.debug {
        wire["debug"] = debug.raw.clone();
    }
    wire
}

fn wire_barcode(barcode: &Barcode) -> serde_json::Value {
    let rect = barcode.rect();
    let mut value = serde_json::json!({
        "text": barcode.text,
        "format": barcode.format.as_str(),
        "polygon": barcode.polygon,
        "support": barcode.support,
        "rect": { "left": rect[0], "top": rect[1], "width": rect[2], "height": rect[3] },
    });
    let object = value.as_object_mut().expect("barcode JSON is an object");
    optional(object, "payloadBytes", barcode.payload_bytes.as_ref());
    optional(object, "eanAddOn", barcode.ean_add_on.as_ref());
    optional(object, "gs1", barcode.gs1.as_ref());
    optional(
        object,
        "readerInitialization",
        barcode.reader_initialization.as_ref(),
    );
    if let Some(append) = &barcode.structured_append {
        object.insert("structuredAppend".into(), wire_append(append));
    }
    value
}

fn optional<T: serde::Serialize>(
    object: &mut serde_json::Map<String, serde_json::Value>,
    name: &str,
    value: Option<&T>,
) {
    if let Some(value) = value {
        object.insert(name.into(), serde_json::json!(value));
    }
}

fn wire_append(append: &StructuredAppend) -> serde_json::Value {
    let mut value = serde_json::json!({ "index": append.index, "count": append.count });
    let object = value.as_object_mut().expect("append JSON is an object");
    optional(object, "id", append.id.as_ref());
    optional(object, "parity", append.parity.as_ref());
    value
}

/// Pointer to the last JSON result/error, valid until the next scan or destroy.
#[no_mangle]
pub extern "C" fn tapirscan_output_ptr(handle: u32) -> u32 {
    REGISTRY.with_borrow(|registry| {
        registry
            .sessions
            .get(&handle)
            .map_or(0, |session| pointer(&session.output))
    })
}

/// Byte length of the last JSON result/error, excluding any terminator.
#[no_mangle]
pub extern "C" fn tapirscan_output_len(handle: u32) -> u32 {
    REGISTRY.with_borrow(|registry| {
        registry
            .sessions
            .get(&handle)
            .and_then(|session| u32::try_from(session.output.len()).ok())
            .unwrap_or(0)
    })
}
