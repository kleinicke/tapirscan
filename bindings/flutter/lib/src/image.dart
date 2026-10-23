import 'dart:typed_data';

/// 8-bit gray (1 channel), RGB (3) or RGBA (4, alpha ignored) pixels.
///
/// [stride] is the distance between rows in bytes; 0 means `width * channels`.
/// [bytes] must hold `(height - 1) * stride + width * channels` bytes, so the
/// last row may be shorter than the stride. A camera frame's luma plane (plane
/// 0 of YUV420, NV21 or iOS bi-planar YUV) is a gray image with its
/// `bytesPerRow` as stride. Each scan copies the pixels into native memory, so
/// the list may be reused once the call returns.
final class PixelImage {
  const PixelImage(
    this.bytes, {
    required this.width,
    required this.height,
    this.channels = 1,
    this.stride = 0,
  });

  /// BGRA pixels, such as an iOS camera frame in `bgra8888`, copied to RGBA.
  /// Throws [ArgumentError] when [bytes] cannot hold the frame.
  factory PixelImage.bgra(
    Uint8List bytes, {
    required int width,
    required int height,
    int stride = 0,
  }) {
    final rowBytes = stride == 0 ? width * 4 : stride;
    if (width < 1 ||
        height < 1 ||
        rowBytes < width * 4 ||
        bytes.length < (height - 1) * rowBytes + width * 4) {
      throw ArgumentError(
        'A ${width}x$height BGRA image needs a row stride of at least '
        '${width * 4} and ${(height - 1) * rowBytes + width * 4} bytes; '
        'got $rowBytes and ${bytes.length}',
      );
    }
    final rgba = Uint8List(width * height * 4);
    for (var y = 0; y < height; y++) {
      var from = y * rowBytes;
      var to = y * width * 4;
      for (var x = 0; x < width; x++, from += 4, to += 4) {
        rgba[to] = bytes[from + 2];
        rgba[to + 1] = bytes[from + 1];
        rgba[to + 2] = bytes[from];
        rgba[to + 3] = 255;
      }
    }
    return PixelImage(rgba, width: width, height: height, channels: 4);
  }

  final Uint8List bytes;
  final int width;
  final int height;
  final int channels;
  final int stride;
}
