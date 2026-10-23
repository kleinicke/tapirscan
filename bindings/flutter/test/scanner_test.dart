// Host tests: decode fixtures through dart:ffi and the build hook's library.
import 'dart:io';
import 'dart:typed_data';

import 'package:image/image.dart' as img;
import 'package:tapirscan/tapirscan.dart';
import 'package:test/test.dart';

const _ean = '4006381333931';

/// Independent EAN-13 writer (as in scripts/fixture_data.py): 480x180 gray.
PixelImage syntheticEan() {
  const l = [
    '0001101', '0011001', '0010011', '0111101', '0100011', //
    '0110001', '0101111', '0111011', '0110111', '0001011',
  ];
  const g = [
    '0100111', '0110011', '0011011', '0100001', '0011101', //
    '0111001', '0000101', '0010001', '0001001', '0010111',
  ];
  final bits = StringBuffer('101');
  for (var i = 0; i < 6; i++) {
    bits.write(('LGLLGG'[i] == 'L' ? l : g)[int.parse(_ean[i + 1])]);
  }
  bits.write('01010');
  for (var i = 7; i < 13; i++) {
    bits.write(
      l[int.parse(_ean[i])].split('').map((b) => b == '0' ? '1' : '0').join(),
    );
  }
  bits.write('101');
  const width = 480, height = 180;
  final data = Uint8List(width * height)..fillRange(0, width * height, 255);
  final pattern = bits.toString();
  for (var y = 30; y < 150; y++) {
    for (var i = 0; i < pattern.length; i++) {
      if (pattern[i] == '1') {
        data.fillRange(y * width + 50 + i * 4, y * width + 54 + i * 4, 0);
      }
    }
  }
  return PixelImage(data, width: width, height: height);
}

const _pesto = '8076809586177';

/// A photographed, rotated EAN-13 label; see test/fixtures/README.md.
final img.Image photo = img.decodePng(
  File('test/fixtures/pesto-label.png').readAsBytesSync(),
)!;

PixelImage rgb(img.Image image) => PixelImage(
  image.convert(numChannels: 3).getBytes(order: img.ChannelOrder.rgb),
  width: image.width,
  height: image.height,
  channels: 3,
);

/// Gray pixels as an Android Y plane: rows padded to [stride] with noise and
/// the last row ending at the image width, shorter than the stride.
Uint8List paddedLuma(PixelImage gray, int stride) {
  final out = Uint8List((gray.height - 1) * stride + gray.width);
  for (var i = 0; i < out.length; i++) {
    out[i] = (i * 131) & 0xff;
  }
  for (var y = 0; y < gray.height; y++) {
    out.setRange(
      y * stride,
      y * stride + gray.width,
      gray.bytes,
      y * gray.width,
    );
  }
  return out;
}

PixelImage luma(img.Image image) {
  final gray = img.grayscale(image.convert(numChannels: 3));
  final bytes = Uint8List(image.width * image.height);
  var i = 0;
  for (final pixel in gray) {
    bytes[i++] = pixel.r.toInt();
  }
  return PixelImage(bytes, width: image.width, height: image.height);
}

void main() {
  late Scanner scanner;
  setUp(() => scanner = Scanner(const ScannerOptions(mode: Mode.medium)));
  tearDown(() => scanner.dispose());

  test('loads the native library through the build hook', () {
    expect(Tapirscan.isSupported, isTrue);
  });

  test('decodes a synthetic EAN-13', () {
    final result = scanner.scanSync(syntheticEan());
    expect(result.values, [_ean]);
    final barcode = result.best!;
    expect(barcode.format, Format.ean13);
    expect(barcode.polygon, hasLength(4));
    expect(barcode.rect.left, inInclusiveRange(40, 60));
    expect(barcode.rect.width, greaterThan(300));
  });

  test('decodes a photographed label from RGB in every mode', () {
    for (final mode in [Mode.low, Mode.medium, Mode.high]) {
      final each = Scanner(ScannerOptions(mode: mode));
      final result = each.scanSync(rgb(photo));
      each.dispose();
      expect(result.values, [_pesto], reason: '$mode');
      expect(result.best!.format, Format.ean13);
    }
  });

  test('reads a camera Y plane with padded rows and a short last row', () {
    final gray = luma(photo);
    expect(scanner.scanSync(gray).values, [_pesto]);
    final stride = gray.width + 64;
    final bytes = paddedLuma(gray, stride);
    expect(bytes.length, lessThan(gray.height * stride));
    final frame = PixelImage(
      bytes,
      width: gray.width,
      height: gray.height,
      stride: stride,
    );
    expect(scanner.scanSync(frame).values, [_pesto]);
  });

  test('reads a padded Y plane of the synthetic barcode', () {
    final gray = syntheticEan();
    final frame = PixelImage(
      paddedLuma(gray, 512),
      width: gray.width,
      height: gray.height,
      stride: 512,
    );
    expect(scanner.scanSync(frame).values, [_ean]);
  });

  test('converts an iOS BGRA frame with row padding', () {
    final source = rgb(photo);
    final stride = photo.width * 4 + 32;
    final bgra = Uint8List(photo.height * stride);
    for (var y = 0; y < photo.height; y++) {
      for (var x = 0; x < photo.width; x++) {
        final from = (y * photo.width + x) * 3;
        final to = y * stride + x * 4;
        bgra[to] = source.bytes[from + 2];
        bgra[to + 1] = source.bytes[from + 1];
        bgra[to + 2] = source.bytes[from];
        bgra[to + 3] = 255;
      }
    }
    final frame = PixelImage.bgra(
      bgra,
      width: photo.width,
      height: photo.height,
      stride: stride,
    );
    expect(scanner.scanSync(frame).values, [_pesto]);
  });

  test('scans RGBA with a row stride and ignores alpha', () {
    final gray = syntheticEan();
    const stride = 480 * 4 + 7;
    final rgba = Uint8List(179 * stride + 480 * 4);
    for (var y = 0; y < 180; y++) {
      for (var x = 0; x < 480; x++) {
        final value = gray.bytes[y * 480 + x];
        rgba.setRange(y * stride + x * 4, y * stride + x * 4 + 4, [
          value,
          value,
          value,
          0,
        ]);
      }
    }
    final image = PixelImage(
      rgba,
      width: 480,
      height: 180,
      channels: 4,
      stride: stride,
    );
    expect(scanner.scanSync(image).values, [_ean]);
  });

  test('scans a crop of a frame through a view and its stride', () {
    final gray = syntheticEan();
    const left = 40, top = 20, width = 400, height = 140;
    final crop = PixelImage(
      Uint8List.sublistView(gray.bytes, top * 480 + left),
      width: width,
      height: height,
      stride: 480,
    );
    final barcode = scanner.scanSync(crop).best!;
    expect(barcode.text, _ean);
    expect(barcode.rect.left, inInclusiveRange(0, 20));
  });

  test('per-call formats restrict one scan', () {
    expect(
      scanner.scanSync(syntheticEan(), formats: {Format.qrCode}).values,
      isEmpty,
    );
    expect(scanner.scanSync(syntheticEan()).values, [_ean]);
  });

  test('a blank image is an empty result', () {
    final blank = Uint8List(64 * 48)..fillRange(0, 64 * 48, 255);
    final result = scanner.scanSync(PixelImage(blank, width: 64, height: 48));
    expect(result.barcodes, isEmpty);
    expect(result.best, isNull);
  });

  test('reads both barcodes of an image with two symbols', () {
    final one = syntheticEan();
    final two = Uint8List(480 * 420)..fillRange(0, 480 * 420, 255);
    two.setRange(0, one.bytes.length, one.bytes);
    two.setRange(240 * 480, 240 * 480 + one.bytes.length, one.bytes);
    final result = scanner.scanSync(PixelImage(two, width: 480, height: 420));
    expect(result.values, [_ean, _ean]);
  });

  test('too small dimensions are invalid input', () {
    expect(
      () => scanner.scanSync(PixelImage(Uint8List(4), width: 2, height: 2)),
      throwsA(
        isA<ScannerException>().having(
          (e) => e.code,
          'code',
          ScannerErrorCode.invalidInput,
        ),
      ),
    );
  });

  test('a short buffer is invalid input', () {
    final image = PixelImage(Uint8List(100), width: 480, height: 180);
    expect(
      () => scanner.scanSync(image),
      throwsA(
        isA<ScannerException>().having(
          (e) => e.code,
          'code',
          ScannerErrorCode.invalidInput,
        ),
      ),
    );
  });

  test('a BGRA frame with too few bytes is rejected before scanning', () {
    expect(
      () => PixelImage.bgra(Uint8List(10), width: 10, height: 10, stride: 40),
      throwsArgumentError,
    );
  });

  test('scans on background isolates concurrently', () async {
    final results = await Future.wait([
      for (var i = 0; i < 4; i++) scanner.scan(syntheticEan()),
    ]);
    expect([for (final r in results) r.values], List.filled(4, [_ean]));
  });

  test('a disposed scanner throws StateError', () {
    final other = Scanner()..dispose();
    expect(() => other.scanSync(syntheticEan()), throwsStateError);
    expect(other.scan(syntheticEan()), throwsStateError);
    other.dispose();
  });

  test('an empty format set is rejected', () {
    expect(
      () => scanner.scanSync(syntheticEan(), formats: {}),
      throwsArgumentError,
    );
    expect(
      () => Scanner(const ScannerOptions(formats: {})),
      throwsArgumentError,
    );
  });

  test('disposing during a background scan is safe', () async {
    final other = Scanner();
    final pending = other.scan(syntheticEan());
    other.dispose();
    try {
      expect((await pending).values, [_ean]);
    } on StateError {
      // The scan reached the native library after dispose.
    }
  });

  test('formats keep their shared names', () {
    expect(Format.qrCode.id, 'QRCode');
    expect(Format.retail, {
      Format.ean13,
      Format.upcA,
      Format.ean8,
      Format.upcE,
    });
  });
}
