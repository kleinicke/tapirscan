package org.tapirscan.android;

import java.nio.ByteBuffer;
import java.util.Arrays;
import java.util.Collections;
import java.util.EnumSet;
import java.util.Optional;
import java.util.Set;

/** Runs the JNI bridge on a desktop JVM: java -Djava.library.path=... JniTest */
public final class JniTest {
    private static final String TEXT = "4006381333931";
    private static final String[] L = {"0001101", "0011001", "0010011", "0111101", "0100011",
        "0110001", "0101111", "0111011", "0110111", "0001011"};
    private static final String[] PARITY = {"LLLLLL", "LLGLGG", "LLGGLG", "LLGGGL", "LGLLGG",
        "LGGLLG", "LGGGLL", "LGLGLG", "LGLGGL", "LGGLGL"};
    static final int MODULE = 3, QUIET = 36, HEIGHT = 120;

    private static String invert(String bits) {
        StringBuilder out = new StringBuilder();
        for (char c : bits.toCharArray()) out.append(c == '0' ? '1' : '0');
        return out.toString();
    }

    static byte[] ean13(int[] size) {
        StringBuilder bits = new StringBuilder("101");
        int first = TEXT.charAt(0) - '0';
        for (int i = 1; i <= 6; i++) {
            String left = L[TEXT.charAt(i) - '0'];
            bits.append(PARITY[first].charAt(i - 1) == 'L' ? left
                    : new StringBuilder(invert(left)).reverse().toString());
        }
        bits.append("01010");
        for (int i = 7; i <= 12; i++) bits.append(invert(L[TEXT.charAt(i) - '0']));
        bits.append("101");
        int width = bits.length() * MODULE + 2 * QUIET;
        byte[] pixels = new byte[width * HEIGHT];
        Arrays.fill(pixels, (byte) 255);
        for (int i = 0; i < bits.length(); i++) {
            if (bits.charAt(i) != '1') continue;
            for (int y = 10; y < HEIGHT - 10; y++) {
                for (int x = 0; x < MODULE; x++) pixels[y * width + QUIET + i * MODULE + x] = 0;
            }
        }
        size[0] = width;
        return pixels;
    }

    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }

    private static Set<Format> only(Format... formats) {
        return EnumSet.copyOf(Arrays.asList(formats));
    }

    public static void main(String[] args) {
        int[] size = new int[1];
        byte[] pixels = ean13(size);
        int width = size[0];
        ScannerOptions low = ScannerOptions.defaults().withMode(Mode.LOW)
                .withFormats(only(Format.EAN13, Format.QR_CODE));
        try (Scanner scanner = new Scanner(low)) {
            check(scanner.options().equals(low), "options");
            ScanResult fromArray = scanner.scan(pixels, width, HEIGHT, 1, 0);
            check(fromArray.values().equals(Collections.singletonList(TEXT)), "array scan " + fromArray);
            Barcode barcode = fromArray.best().get();
            check(barcode.text().equals(TEXT) && barcode.format() == Format.EAN13, "format");
            check("EAN13".equals(barcode.format().toString()), "format name");
            check(barcode.polygon().size() == 4, "polygon");
            Rect rect = barcode.rect();
            check(rect.left() >= 0 && rect.width() > 0 && rect.height() > 0
                    && rect.left() + rect.width() <= width && rect.top() + rect.height() <= HEIGHT, "rect " + rect);
            check(!barcode.eanAddOn().isPresent() && !barcode.structuredAppend().isPresent(), "optional fields");
            check(!Tapirscan.parseContent(barcode).isPresent(), "retail text has no content");
            // A direct buffer with padded rows, as camera planes have, is read in place.
            int stride = width + 16;
            ByteBuffer direct = ByteBuffer.allocateDirect(stride * HEIGHT + 5);
            direct.position(5);
            for (int y = 0; y < HEIGHT; y++) {
                direct.position(5 + y * stride);
                direct.put(pixels, y * width, width);
            }
            direct.position(5);
            ScanResult fromDirect = scanner.scan(direct, width, HEIGHT, 1, stride);
            check(fromDirect.equals(fromArray), "direct scan " + fromDirect);
            check(scanner.scan(new byte[width * HEIGHT], width, HEIGHT, 1, 0).barcodes().isEmpty(), "blank");
            check(!scanner.scan(new byte[width * HEIGHT], width, HEIGHT, 1, 0).best().isPresent(), "blank best");

            InspectionResult inspection = scanner.inspect(pixels, width, HEIGHT, 1, 0);
            check(inspection.barcodes().equals(fromArray.barcodes()), "inspect barcodes");
            check(inspection.width() == width && inspection.height() == HEIGHT, "inspect size");
            check(inspection.mode() == Mode.LOW && inspection.elapsedMs() >= 0, "inspect mode");
            check(inspection.diagnostics().startsWith("{"), "diagnostics");
            check(inspection.values().equals(fromArray.values()), "inspect values");
            check(scanner.inspect(direct, width, HEIGHT, 1, stride).barcodes().equals(fromArray.barcodes()),
                    "direct inspect");
            // tapirscan_parallel: 0 is the default, so zeroed native options keep it.
            check(Scanner.parallelCode(null) == 0 && Scanner.parallelCode(false) == 1
                    && Scanner.parallelCode(true) == 2, "parallel codes");
            for (boolean parallel : new boolean[] {false, true}) {
                try (Scanner threads = new Scanner(low.withParallel(parallel))) {
                    check(threads.scan(pixels, width, HEIGHT, 1, 0).equals(fromArray), "parallel " + parallel);
                }
            }

            try {
                scanner.scan(new byte[2], 3, 3, 1, 0);
                throw new AssertionError("short buffer accepted");
            } catch (ScannerException expected) {
                check(!expected.getMessage().isEmpty() && expected.code == 1, "message");
                check("invalid_input".equals(expected.codeName()), "code name");
            }
            try {
                scanner.scan(new byte[2], -3, 3, 1, 0);
                throw new AssertionError("negative width accepted");
            } catch (ScannerException expected) {
                check(expected.code == 1, "negative width code");
            }
        }
        // A per-call selection overrides the scanner's formats for that call only.
        try (Scanner qr = new Scanner(ScannerOptions.defaults().withFormats(only(Format.QR_CODE)))) {
            check(qr.scan(pixels, width, HEIGHT, 1, 0).barcodes().isEmpty(), "qr scanner ignores EAN-13");
            ScanOptions ean = ScanOptions.defaults().withFormats(only(Format.EAN13));
            check(qr.scan(pixels, width, HEIGHT, 1, 0, ean).values().equals(Collections.singletonList(TEXT)),
                    "override");
            check(qr.scan(pixels, width, HEIGHT, 1, 0).barcodes().isEmpty(), "override is per call");
            try {
                new ScanOptions(Optional.of(Collections.<Format>emptySet()));
                throw new AssertionError("empty override accepted");
            } catch (IllegalArgumentException expected) {
                // formats must be nonempty
            }
        }
        Scanner closed = new Scanner();
        closed.close();
        closed.close();
        try {
            closed.scan(pixels, width, HEIGHT, 1, 0);
            throw new AssertionError("closed scanner accepted a scan");
        } catch (IllegalStateException expected) {
            // use after close
        }
        try {
            new ScannerOptions(Mode.MEDIUM, Collections.<Format>emptySet(), EanAddOnPolicy.IGNORE);
            throw new AssertionError("empty formats accepted");
        } catch (IllegalArgumentException expected) {
            // formats must be nonempty
        }
        // Presets and opt-in formats.
        check(Format.LINEAR.contains(Format.CODE93) && !Format.LINEAR.contains(Format.QR_CODE), "linear preset");
        check(Format.MATRIX.contains(Format.AZTEC) && !Format.MATRIX.contains(Format.EAN13), "matrix preset");
        check(!Format.ALL.contains(Format.PZN) && Format.ALL.contains(Format.MAXI_CODE), "all preset");
        new Scanner(new ScannerOptions(Mode.LOW, only(Format.CODE32, Format.PZN, Format.MSI_PLESSEY,
                Format.INDUSTRIAL25, Format.PHARMACODE),
                EanAddOnPolicy.READ)).close();
        Content wifi = Tapirscan.parseContent("WIFI:S:Café;T:WPA;P:secret;;").get();
        check(wifi.kind() == ContentKind.WIFI && "WiFi".equals(wifi.kind().toString()), "wifi kind");
        check("Café".equals(wifi.get("ssid").get()) && !wifi.get("hidden").isPresent()
                && wifi.fields().size() == 3, "wifi fields");
        check(Tapirscan.parseContent("0109506000134352", true).get().getAll("01")
                .equals(Collections.singletonList("09506000134352")), "gs1");
        check(!Tapirscan.parseContent("0109506000134352").isPresent(), "plain digits");
        System.out.println("android JNI bridge: OK");
    }
}
