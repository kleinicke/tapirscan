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

    public static void main(String[] args) {
        byte[] bytes = {0, 1, -1};
        Barcode a = barcode(bytes, 5), b = barcode(bytes.clone(), 5);
        check(a.equals(b) && a.hashCode() == b.hashCode());
        bytes[0] = 99;
        a.payloadBytes().orElseThrow()[0] = 88;
        check(a.equals(b));
        check(!a.equals(barcode(new byte[]{1}, 5)));
        ScanResult result = new ScanResult(List.of(a, b), List.of(), 3, 3, Mode.MEDIUM,
                0, false, "{}");
        check(result.best().orElseThrow() == a);
        check(Native.IMAGE.byteSize() == 48 && Native.SUMMARY.byteSize() == 56);
        check(Native.BARCODE.byteSize() == 136 && Native.REGION.byteSize() == 72);
        check(Native.SCANNER_OPTIONS.byteSize() == 12 && Native.SCAN_OPTIONS.byteSize() == 8);
        check(Native.offset(Native.IMAGE, "stride") == 40);
        check(Native.offset(Native.SUMMARY, "mode") == 48);
        check(Native.offset(Native.BARCODE, "appendIdLength") == 128);
        try (Scanner scanner = new Scanner()) {
            try {
                scanner.scan(Image.gray(new byte[1], 3, 3));
                throw new AssertionError("Short input accepted");
            } catch (ScannerException error) {
                check(error.code == 1 && error.getMessage().contains("buffer"));
            }
        }
    }
}
