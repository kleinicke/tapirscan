package org.tapirscan;

import java.util.List;
import java.util.Optional;

/** Value semantics and the named C layouts, independent of image fixtures. */
public final class ApiTest {
    private static void check(boolean value) {
        if (!value) throw new AssertionError("API contract failed");
    }

    private static Barcode barcode(byte[] bytes, long support) {
        return new Barcode("value", Format.QR_CODE,
                List.of(new Point(1, 2), new Point(3, 2), new Point(3, 4), new Point(1, 4)),
                support, Optional.of(bytes), Optional.empty(), Optional.empty(), Optional.empty(), Optional.empty());
    }

    public static void main(String[] args) throws InterruptedException {
        byte[] bytes = {0, 1, -1};
        Barcode a = barcode(bytes, 5), b = barcode(bytes.clone(), 5);
        check(a.equals(b) && a.hashCode() == b.hashCode());
        bytes[0] = 99;
        a.payloadBytes().orElseThrow()[0] = 88;
        check(a.equals(b));
        check(!a.equals(barcode(new byte[]{1}, 5)));
        var source = new java.util.ArrayList<>(List.of(a, b));
        var compact = new ScanResult(source);
        source.clear();
        check(compact.values().equals(List.of("value", "value")));
        check(compact.best().orElseThrow() == a);
        check(new ScanResult(List.of()).best().isEmpty());
        InspectionResult result = new InspectionResult(List.of(a, b), List.of(), 3, 3, Mode.MEDIUM, 0, "{}");
        check(result.best().orElseThrow() == a);
        Barcode stronger = barcode(new byte[]{2}, 6);
        check(new ScanResult(List.of(a, stronger, b)).best().orElseThrow() == stronger);
        check(new ScanResult(List.of()).best().isEmpty());
        check(Native.IMAGE.byteSize() == 48 && Native.SUMMARY.byteSize() == 48);
        check(Native.BARCODE.byteSize() == 136 && Native.REGION.byteSize() == 72);
        check(Native.SCANNER_OPTIONS.byteSize() == 16 && Native.SCAN_OPTIONS.byteSize() == 4);
        check(Native.offset(Native.IMAGE, "stride") == 40);
        check(Native.CONTENT_SUMMARY.byteSize() == 16 && Native.CONTENT_LENGTHS.byteSize() == 16);
        Content wifi = Tapirscan.parseContent("WIFI:S:Café;T:WPA;P:secret;;", false).orElseThrow();
        check(wifi.kind() == ContentKind.WIFI && wifi.kind().toString().equals("WiFi"));
        check(wifi.get("ssid").equals(Optional.of("Café")) && wifi.get("hidden").isEmpty());
        check(wifi.fields().size() == 3 && wifi.getAll("password").equals(List.of("secret")));
        Barcode gs1 = new Barcode("0109506000134352", Format.CODE128, a.polygon(), 1, Optional.empty(),
                Optional.empty(), Optional.of(true), Optional.empty(), Optional.empty());
        check(Tapirscan.parseContent(gs1).orElseThrow().get("01").equals(Optional.of("09506000134352")));
        check(Tapirscan.parseContent(gs1.text(), false).isEmpty());
        check(Tapirscan.parseContent("", false).isEmpty());
        check(Native.offset(Native.SUMMARY, "mode") == 40);
        for (Mode mode : Mode.values()) check(Mode.fromCode(mode.code()) == mode);
        check(EanAddOnPolicy.REQUIRE.code() == 2);
        // Only the addressed rows are copied, and never more than the native layout limit.
        check(Image.gray(new byte[100], 3, 3).withStride(10).addressedBytes() == 23);
        check(Image.gray(new byte[4], 3, 3).addressedBytes() == 4);
        try {
            Image.gray(new byte[9], -3, 3);
            throw new AssertionError("Negative width accepted");
        } catch (IllegalArgumentException expected) {
            // Native fields are unsigned.
        }
        check(Native.offset(Native.BARCODE, "appendIdLength") == 128);
        try (Scanner scanner = new Scanner()) {
            try {
                scanner.scan(Image.gray(new byte[1], 3, 3));
                throw new AssertionError("Short input accepted");
            } catch (ScannerException error) {
                check(error.code == 1 && error.getMessage().contains("buffer"));
            }
            // Zero-copy segments must be open and usable on the scanning thread.
            java.lang.foreign.MemorySegment closed;
            try (java.lang.foreign.Arena arena = java.lang.foreign.Arena.ofConfined()) {
                closed = arena.allocate(9);
            }
            try {
                scanner.scan(new Image(closed, 3, 3, 1, 3));
                throw new AssertionError("Closed segment accepted");
            } catch (IllegalStateException expected) {
                check(expected.getMessage().contains("closed"));
            }
            java.lang.foreign.MemorySegment[] foreign = new java.lang.foreign.MemorySegment[1];
            Thread owner = new Thread(() -> foreign[0] = java.lang.foreign.Arena.ofConfined().allocate(9));
            owner.start();
            owner.join();
            try {
                scanner.scan(new Image(foreign[0], 3, 3, 1, 3));
                throw new AssertionError("Foreign-thread segment accepted");
            } catch (IllegalStateException expected) {
                check(expected.getMessage().contains("thread"));
            }
            // Heap pixels share one reused native buffer: a larger image grows it, a smaller
            // one reuses it, and another thread may scan with the same scanner in turn.
            check(scanner.scan(Image.gray(new byte[64 * 48], 64, 48)).barcodes().isEmpty());
            check(scanner.scan(Image.gray(new byte[9], 3, 3)).barcodes().isEmpty());
            boolean[] scanned = new boolean[1];
            Thread other = new Thread(() ->
                    scanned[0] = scanner.scan(Image.gray(new byte[128 * 96], 128, 96)).barcodes().isEmpty());
            other.start();
            other.join();
            check(scanned[0]);
            check(scanner.scan(Image.gray(new byte[9], 3, 3)).barcodes().isEmpty());
        }
    }
}
