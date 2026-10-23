/// Barcode scanning with Tapirscan through `dart:ffi`.
library;

export 'src/format.g.dart' show Format;
export 'src/image.dart';
export 'src/result.dart';
export 'src/scanner.dart'
    show
        EanAddOnPolicy,
        Mode,
        Scanner,
        ScannerErrorCode,
        ScannerException,
        ScannerOptions,
        Tapirscan;
