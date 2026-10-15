package org.tapirscan.android;

import java.nio.ByteBuffer;
import java.util.List;

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
        java.util.Arrays.fill(pixels, (byte) 255);
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

    public static void main(String[] args) {
        int[] size = new int[1];
        byte[] pixels = ean13(size);
        int width = size[0];
        try (BarcodeScanner scanner = new BarcodeScanner(BarcodeScanner.MODE_LOW, Formats.EAN13 | Formats.QR_CODE)) {
            List<Barcode> fromArray = scanner.scan(pixels, width, HEIGHT, 1, 0);
            check(fromArray.size() == 1 && fromArray.get(0).text().equals(TEXT), "array scan " + fromArray);
            check(fromArray.get(0).format().equals("EAN13"), "format");
            check(fromArray.get(0).polygon().length == 8, "polygon");
            check(fromArray.get(0).content() == null, "retail text has no content");
            // A direct buffer with padded rows, as camera planes have, is read in place.
            int stride = width + 16;
            ByteBuffer direct = ByteBuffer.allocateDirect(stride * HEIGHT + 5);
            direct.position(5);
            for (int y = 0; y < HEIGHT; y++) {
                direct.position(5 + y * stride);
                direct.put(pixels, y * width, width);
            }
            direct.position(5);
            List<Barcode> fromDirect = scanner.scan(direct, width, HEIGHT, 1, stride);
            check(fromDirect.size() == 1 && fromDirect.get(0).text().equals(TEXT), "direct scan " + fromDirect);
            check(scanner.scan(new byte[width * HEIGHT], width, HEIGHT, 1, 0).isEmpty(), "blank");
            try {
                scanner.scan(new byte[2], 3, 3, 1, 0);
                throw new AssertionError("short buffer accepted");
            } catch (TapirscanException expected) {
                check(!expected.getMessage().isEmpty(), "message");
            }
        }
        try {
            new BarcodeScanner(BarcodeScanner.MODE_MEDIUM, 0);
            throw new AssertionError("empty formats accepted");
        } catch (TapirscanException expected) {
            // formats must be nonempty
        }
        Content wifi = Content.parse("WIFI:S:Café;T:WPA;P:secret;;", false);
        check(wifi != null && wifi.kind() == Content.Kind.WIFI && "WiFi".equals(wifi.kind().toString()), "wifi kind");
        check("Café".equals(wifi.get("ssid")) && wifi.get("hidden") == null && wifi.fields().size() == 3, "wifi fields");
        check(Content.parse("0109506000134352", true).getAll("01").equals(List.of("09506000134352")), "gs1");
        check(Content.parse("0109506000134352", false) == null, "plain digits");
        System.out.println("android JNI bridge: OK");
    }
}
