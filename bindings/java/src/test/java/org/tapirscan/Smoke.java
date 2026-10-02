package org.tapirscan;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import java.util.Set;

/** Scan a raw pixel file and print the typed result as JSON, like the C++ example. */
public final class Smoke {
    private static String quoted(String text) {
        StringBuilder out = new StringBuilder("\"");
        for (char c : text.toCharArray()) {
            if (c == '"' || c == '\\') out.append('\\').append(c);
            else if (c < 32) out.append(String.format(Locale.ROOT, "\\u%04x", (int) c));
            else out.append(c);
        }
        return out.append('"').toString();
    }

    private static String polygon(List<Point> points) {
        StringBuilder out = new StringBuilder("[");
        for (Point p : points) {
            if (out.length() > 1) out.append(',');
            out.append('[').append(p.x()).append(',').append(p.y()).append(']');
        }
        return out.append(']').toString();
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 6 && args.length != 8) {
            throw new IllegalArgumentException("mode width height channels stride pixels [debug formats]");
        }
        Mode mode = Mode.valueOf(args[0].replace('-', '_').toUpperCase(Locale.ROOT));
        byte[] pixels = Files.readAllBytes(Path.of(args[5]));
        Image image = new Image(pixels, Integer.parseInt(args[1]), Integer.parseInt(args[2]),
                Integer.parseInt(args[3]), Integer.parseInt(args[4]));
        ScanOptions options = ScanOptions.defaults();
        if (args.length == 8) {
            int mask = Integer.parseInt(args[7]);
            Set<Format> formats = Set.copyOf(Format.ALL.stream().filter(f -> (mask & f.bit()) != 0).toList());
            options = options.withDebug(args[6].equals("1")).withFormats(formats);
        }
        Scanner scanner = new Scanner(ScannerOptions.defaults().withMode(mode));
        ScanResult result;
        try (scanner) {
            result = scanner.scan(image, options);
            try {
                scanner.scan(Image.gray(new byte[1], image.width(), image.height()));
                throw new AssertionError("Short input accepted");
            } catch (ScannerException expected) {
                if (expected.code != 1) throw expected;
            }
        }
        scanner.close();
        try {
            scanner.scan(image);
            throw new AssertionError("Closed scanner accepted");
        } catch (IllegalStateException expected) {
            // Results own their data and remain usable after closing.
        }
        if (result.mode() != mode) throw new AssertionError("Mode was not applied");

        StringBuilder out = new StringBuilder();
        out.append("{\"mode\":").append(quoted(result.mode().toString()))
                .append(",\"unfinished\":").append(result.unfinished())
                .append(",\"best\":").append(result.bestIndex().isPresent() ? result.bestIndex().getAsInt() : "null")
                .append(",\"barcodes\":[");
        for (int i = 0; i < result.barcodes().size(); i++) {
            Barcode b = result.barcodes().get(i);
            out.append(i == 0 ? "" : ",").append("{\"text\":").append(quoted(b.text()))
                    .append(",\"format\":").append(quoted(b.format().toString()))
                    .append(",\"support\":").append(b.support())
                    .append(",\"polygon\":").append(polygon(b.polygon())).append('}');
        }
        out.append("],\"undecoded\":[");
        for (int i = 0; i < result.undecoded().size(); i++) {
            UndecodedRegion r = result.undecoded().get(i);
            out.append(i == 0 ? "" : ",").append("{\"format\":")
                    .append(quoted(r.format().map(Format::toString).orElse("Unknown")))
                    .append(",\"polygon\":").append(polygon(r.polygon())).append('}');
        }
        out.append("],\"debug\":").append(result.debug().orElse("null")).append('}');
        System.out.println(out);
    }
}
