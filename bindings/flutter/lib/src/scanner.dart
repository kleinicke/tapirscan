import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'format.g.dart';
import 'image.dart';
import 'result.dart';

/// Effort mode, fixed when a scanner is created.
enum Mode {
  low(0),
  medium(1),
  high(2),
  veryHigh(3);

  const Mode(this._native);
  final int _native;
}

/// Whether to read the adjacent two/five-digit EAN/UPC supplement. `require`
/// returns retail reads only with a readable supplement.
enum EanAddOnPolicy {
  ignore(0),
  read(1),
  require(2);

  const EanAddOnPolicy(this._native);
  final int _native;
}

/// Scanner configuration, fixed when the scanner is created.
final class ScannerOptions {
  const ScannerOptions({
    this.mode = Mode.medium,
    this.formats = Format.retail,
    this.eanAddOnPolicy = EanAddOnPolicy.ignore,
    this.parallel,
  });

  final Mode mode;
  final Set<Format> formats;
  final EanAddOnPolicy eanAddOnPolicy;

  /// Whether one scan runs its formats on separate threads; null uses the
  /// process default.
  final bool? parallel;
}

/// Why a call failed, named as in the other bindings (`invalid_input`,
/// `engine`). A disposed scanner throws [StateError] instead.
enum ScannerErrorCode {
  /// Invalid pixels, dimensions or options (C status 1).
  invalidInput,

  /// Any other native failure.
  engine,
}

/// A failed call: [code], the C [status] and the library's [message].
final class ScannerException implements Exception {
  ScannerException._(this.status, this.message);

  final int status;
  final String message;

  ScannerErrorCode get code =>
      status == tapirscan_status.TAPIRSCAN_INVALID_ARGUMENT
      ? ScannerErrorCode.invalidInput
      : ScannerErrorCode.engine;

  @override
  String toString() =>
      'ScannerException(${code.name}): $message (status $status)';
}

/// Library-wide information.
abstract final class Tapirscan {
  static bool? _supported;

  /// Whether this platform has a compatible native library. False on targets
  /// without one, such as 32-bit Android, where creating a [Scanner] throws
  /// [UnsupportedError] instead of crashing.
  static bool get isSupported => _supported ??= _probe();

  static bool _probe() {
    try {
      return tapirscan_abi_version() == TAPIRSCAN_ABI_VERSION;
    } on ArgumentError {
      // The native asset is missing for this target.
      return false;
    }
  }
}

final _finalizer = Finalizer<int>(tapirscan_scanner_destroy);

/// Scanners with a [Scanner.scan] in progress, kept reachable so the finalizer
/// cannot release a native handle that a background isolate is using.
final _scanning = <Scanner>[];

/// A reusable barcode scanner. Create one and reuse it for every frame; calls
/// on one scanner run one at a time, and separate scanners run concurrently.
final class Scanner {
  Scanner([this.options = const ScannerOptions()]) {
    if (!Tapirscan.isSupported) {
      throw UnsupportedError(
        'Tapirscan has no compatible native library for this platform '
        '(64-bit Android, iOS, macOS, Linux and Windows are supported; a '
        'prebuilt library must match native ABI $TAPIRSCAN_ABI_VERSION)',
      );
    }
    _handle = using((arena) {
      final native = arena<tapirscan_scanner_options>();
      native.ref
        ..mode = options.mode._native
        ..formats = _mask(options.formats)
        ..ean_add_on_policy = options.eanAddOnPolicy._native
        ..parallel = switch (options.parallel) {
          null => tapirscan_parallel.TAPIRSCAN_PARALLEL_DEFAULT,
          false => tapirscan_parallel.TAPIRSCAN_PARALLEL_OFF,
          true => tapirscan_parallel.TAPIRSCAN_PARALLEL_ON,
        };
      final out = arena<Uint64>();
      final error = arena<tapirscan_error>();
      _check(tapirscan_scanner_create(native, out, error), error);
      return out.value;
    });
    _finalizer.attach(this, _handle, detach: this);
  }

  final ScannerOptions options;
  late final int _handle;
  bool _disposed = false;

  /// Whether [dispose] has been called.
  bool get isDisposed => _disposed;

  /// Scan [image] on a background isolate, so the calling isolate stays
  /// responsive. [formats], if given, replaces the scanner's formats for this
  /// call. No detection is an empty result. Scans on one scanner run one at a
  /// time. A scan already running when [dispose] is called completes; scans
  /// that start after it throw [StateError].
  Future<ScanResult> scan(PixelImage image, {Set<Format>? formats}) async {
    final handle = _live;
    final mask = formats == null ? 0 : _mask(formats);
    _scanning.add(this);
    try {
      return await Isolate.run(() => _scan(handle, image, mask));
    } finally {
      _scanning.remove(this);
    }
  }

  /// Like [scan], on the calling isolate. Blocks it for the scan's duration.
  ScanResult scanSync(PixelImage image, {Set<Format>? formats}) =>
      _scan(_live, image, formats == null ? 0 : _mask(formats));

  /// Release the native scanner. Later calls throw [StateError].
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _finalizer.detach(this);
    tapirscan_scanner_destroy(_handle);
  }

  int get _live {
    if (_disposed) throw _disposedError();
    return _handle;
  }
}

StateError _disposedError() => StateError('The scanner has been disposed');

int _mask(Set<Format> formats) {
  if (formats.isEmpty) {
    throw ArgumentError.value(formats, 'formats', 'must not be empty');
  }
  return formatMask(formats);
}

void _check(int status, Pointer<tapirscan_error>? error) {
  if (status == tapirscan_status.TAPIRSCAN_OK) return;
  if (status == tapirscan_status.TAPIRSCAN_INVALID_HANDLE) {
    throw _disposedError();
  }
  String? message;
  if (error != null) {
    final bytes = <int>[];
    for (var i = 0; i < 512 && error.ref.message[i] != 0; i++) {
      bytes.add(error.ref.message[i] & 0xff);
    }
    message = utf8.decode(bytes, allowMalformed: true);
  }
  throw ScannerException._(
    status,
    message == null || message.isEmpty
        ? tapirscan_status_message(status).cast<Utf8>().toDartString()
        : message,
  );
}

ScanResult _scan(int scanner, PixelImage image, int formats) => using((arena) {
  final length = image.bytes.length;
  final pixels = arena<Uint8>(length == 0 ? 1 : length);
  pixels.asTypedList(length).setAll(0, image.bytes);
  final native = arena<tapirscan_image>();
  native.ref
    ..data = pixels
    ..length = length
    ..width = image.width
    ..height = image.height
    ..channels = image.channels
    ..stride = image.stride;
  final options = arena<tapirscan_scan_options>();
  options.ref.formats = formats;
  final out = arena<Uint64>();
  final error = arena<tapirscan_error>();
  _check(tapirscan_scan(scanner, native, options, out, error), error);
  final result = out.value;
  try {
    return _read(result, arena);
  } finally {
    tapirscan_result_destroy(result);
  }
});

ScanResult _read(int result, Arena arena) {
  final count = arena<Uint64>();
  _check(tapirscan_result_count(result, count), null);
  final native = arena<tapirscan_barcode>();
  final barcodes = <Barcode>[];
  for (var i = 0; i < count.value; i++) {
    _check(tapirscan_result_barcode(result, i, native), null);
    final b = native.ref;
    Uint8List? field(int field, int length) {
      if (length == TAPIRSCAN_ABSENT) return null;
      final buffer = malloc<Uint8>(length + 1);
      try {
        _check(
          tapirscan_result_copy(result, i, field, buffer, length + 1),
          null,
        );
        return Uint8List.fromList(
          buffer.asTypedList(length),
        ).asUnmodifiableView();
      } finally {
        malloc.free(buffer);
      }
    }

    String? text(int kind, int length) {
      final bytes = field(kind, length);
      return bytes == null ? null : utf8.decode(bytes, allowMalformed: true);
    }

    bool? flag(int value) => value < 0 ? null : value == 1;
    final format = formatFromBit(b.format);
    barcodes.add(
      Barcode(
        text: text(tapirscan_field.TAPIRSCAN_FIELD_TEXT, b.text_length) ?? '',
        format: format,
        polygon: List.unmodifiable([
          for (var p = 0; p < 4; p++)
            PixelPoint(b.polygon[p].x, b.polygon[p].y),
        ]),
        support: b.support,
        payloadBytes: field(
          tapirscan_field.TAPIRSCAN_FIELD_PAYLOAD_BYTES,
          b.payload_bytes_length,
        ),
        eanAddOn: text(
          tapirscan_field.TAPIRSCAN_FIELD_EAN_ADD_ON,
          b.ean_add_on_length,
        ),
        gs1: flag(b.gs1),
        readerInitialization: flag(b.reader_initialization),
        structuredAppend: b.structured_append_count == 0
            ? null
            : StructuredAppend(
                index: b.structured_append_index,
                count: b.structured_append_count,
                id: text(
                  tapirscan_field.TAPIRSCAN_FIELD_STRUCTURED_APPEND_ID,
                  b.structured_append_id_length,
                ),
                parity: b.structured_append_parity < 0
                    ? null
                    : b.structured_append_parity,
              ),
        codabarStartStop: text(
          tapirscan_field.TAPIRSCAN_FIELD_CODABAR_START_STOP,
          format == Format.codabar ? 2 : TAPIRSCAN_ABSENT,
        ),
      ),
    );
  }
  return ScanResult(List.unmodifiable(barcodes));
}
