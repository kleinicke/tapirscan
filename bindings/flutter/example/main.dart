// Scan an image file: dart run example/main.dart photo.jpg
//
// package:image decodes the file here; a Flutter app can decode with dart:ui
// or scan camera frames instead, as the README shows.
import 'dart:io';

import 'package:image/image.dart' as img;
import 'package:tapirscan/tapirscan.dart';

Future<void> main(List<String> args) async {
  if (args.length != 1) {
    stderr.writeln('Usage: dart run example/main.dart IMAGE');
    exitCode = 64;
    return;
  }
  final image = img.decodeImage(await File(args.single).readAsBytes());
  if (image == null) {
    stderr.writeln('Cannot decode ${args.single}');
    exitCode = 65;
    return;
  }
  final scanner = Scanner(const ScannerOptions(formats: Format.all));
  try {
    final result = await scanner.scan(
      PixelImage(
        image.convert(numChannels: 3).getBytes(order: img.ChannelOrder.rgb),
        width: image.width,
        height: image.height,
        channels: 3,
      ),
    );
    for (final barcode in result.barcodes) {
      print('${barcode.format.name} ${barcode.text} at ${barcode.rect}');
    }
    if (result.barcodes.isEmpty) print('No barcode found');
  } finally {
    scanner.dispose();
  }
}
