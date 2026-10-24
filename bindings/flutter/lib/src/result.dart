import 'dart:typed_data';

import 'format.g.dart';

/// A point in source-image pixels, origin top-left, x rightward and y downward.
final class PixelPoint {
  const PixelPoint(this.x, this.y);

  final double x;
  final double y;

  @override
  bool operator ==(Object other) =>
      other is PixelPoint && other.x == x && other.y == y;

  @override
  int get hashCode => Object.hash(x, y);

  @override
  String toString() => 'PixelPoint($x, $y)';
}

/// Enclosing integer pixel bounds of a polygon.
final class PixelRect {
  const PixelRect(this.left, this.top, this.width, this.height);

  final int left;
  final int top;
  final int width;
  final int height;

  @override
  bool operator ==(Object other) =>
      other is PixelRect &&
      other.left == left &&
      other.top == top &&
      other.width == width &&
      other.height == height;

  @override
  int get hashCode => Object.hash(left, top, width, height);

  @override
  String toString() => 'PixelRect($left, $top, $width, $height)';
}

/// Multipart sequence metadata. The index is one-based; sequences are not assembled.
final class StructuredAppend {
  const StructuredAppend({
    required this.index,
    required this.count,
    this.id,
    this.parity,
  });

  final int index;
  final int count;
  final String? id;
  final int? parity;
}

/// A decoded physical instance. Equal values at distinct locations remain separate.
final class Barcode {
  const Barcode({
    required this.text,
    required this.format,
    required this.polygon,
    required this.support,
    this.payloadBytes,
    this.eanAddOn,
    this.gs1,
    this.readerInitialization,
    this.structuredAppend,
    this.codabarStartStop,
  });

  final String text;
  final Format format;

  /// Four corners in source-image pixels.
  final List<PixelPoint> polygon;

  /// Uncalibrated, reader-specific evidence used by [ScanResult.best]; compare
  /// values, do not interpret them.
  final int support;

  /// Original decoded bytes where the reader reports them.
  final Uint8List? payloadBytes;

  /// The adjacent EAN/UPC supplement, when read.
  final String? eanAddOn;

  /// Whether the reader identified GS1 data; null when it does not report it.
  final bool? gs1;

  /// Whether the symbol programs the reader; null when the reader does not report it.
  final bool? readerInitialization;

  final StructuredAppend? structuredAppend;

  /// Codabar start and stop characters (A to D) in reading order, such as
  /// `"AB"`; [text] omits them. Null for other formats.
  final String? codabarStartStop;

  /// Enclosing integer pixel bounds, from floor(minimum) to ceil(maximum).
  PixelRect get rect {
    final xs = polygon.map((p) => p.x);
    final ys = polygon.map((p) => p.y);
    final left = xs.reduce((a, b) => a < b ? a : b).floor();
    final top = ys.reduce((a, b) => a < b ? a : b).floor();
    return PixelRect(
      left,
      top,
      xs.reduce((a, b) => a > b ? a : b).ceil() - left,
      ys.reduce((a, b) => a > b ? a : b).ceil() - top,
    );
  }

  @override
  String toString() => 'Barcode(${format.id}, $text)';
}

/// Decoded values and their source-image locations, valid after the scanner is disposed.
final class ScanResult {
  const ScanResult(this.barcodes);

  final List<Barcode> barcodes;

  /// Decoded text in scanner order, including repeated values.
  List<String> get values => [for (final barcode in barcodes) barcode.text];

  /// Highest support, keeping the first read on ties; null when nothing was decoded.
  Barcode? get best {
    Barcode? winner;
    for (final barcode in barcodes) {
      if (winner == null || barcode.support > winner.support) winner = barcode;
    }
    return winner;
  }
}
