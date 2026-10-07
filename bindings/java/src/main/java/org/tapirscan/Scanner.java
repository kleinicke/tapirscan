package org.tapirscan;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static org.tapirscan.Native.offset;
import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_DOUBLE;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;
import java.util.Optional;
import java.util.OptionalInt;

/**
 * A reusable scanner. Create it once, scan many images, then close it.
 * Scans on one scanner serialize; results remain valid after {@link #close()}.
 */
public final class Scanner implements AutoCloseable {
    private static final int TEXT = 0, PAYLOAD_BYTES = 1, EAN_ADD_ON = 2, STRUCTURED_APPEND_ID = 3;
    private final Native lib = Native.get();
    private final ScannerOptions options;
    private long handle;

    /** Medium effort, Retail formats and ignored supplements. */
    public Scanner() {
        this(ScannerOptions.defaults());
    }

    public Scanner(ScannerOptions options) {
        this.options = Objects.requireNonNull(options);
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment config = arena.allocate(Native.SCANNER_OPTIONS);
            config.set(JAVA_INT, offset(Native.SCANNER_OPTIONS, "mode"), options.mode().code());
            config.set(JAVA_INT, offset(Native.SCANNER_OPTIONS, "formats"), Format.mask(options.formats()));
            config.set(JAVA_INT, offset(Native.SCANNER_OPTIONS, "addonPolicy"), options.eanAddOnPolicy().code());
            MemorySegment out = arena.allocate(JAVA_LONG);
            MemorySegment error = arena.allocate(Native.ERROR);
            lib.check(Native.call(lib.create, config, out, error), error);
            handle = out.get(JAVA_LONG, 0);
        }
    }

    public ScannerOptions options() {
        return options;
    }

    public ScanResult scan(Image image) {
        return scan(image, ScanOptions.defaults());
    }

    /** Scan one image. No detection is a successful empty result. */
    public ScanResult scan(Image image, ScanOptions scan) {
        return run(image, scan, false, (arena, result) -> new ScanResult(readBarcodes(arena, result)));
    }

    /** Inspect one image, including work status, unread regions and diagnostics. */
    public InspectionResult inspect(Image image) {
        return inspect(image, ScanOptions.defaults());
    }

    public InspectionResult inspect(Image image, ScanOptions scan) {
        return run(image, scan, true, this::read);
    }

    private synchronized <T> T run(Image image, ScanOptions scan, boolean inspect,
            java.util.function.BiFunction<Arena, Long, T> reader) {
        Objects.requireNonNull(image);
        Objects.requireNonNull(scan);
        if (handle == 0) throw new IllegalStateException("Scanner is closed");
        long length = image.addressedBytes();
        try (Arena arena = Arena.ofConfined()) {
            // Native code cannot read the Java heap: copy only the addressed bytes.
            MemorySegment pixels = image.pixels().isNative() ? image.pixels()
                    : arena.allocate(Math.max(length, 1)).copyFrom(image.pixels().asSlice(0, length));
            MemorySegment input = arena.allocate(Native.IMAGE);
            input.set(ADDRESS, offset(Native.IMAGE, "data"), pixels);
            input.set(JAVA_LONG, offset(Native.IMAGE, "length"), length);
            input.set(JAVA_LONG, offset(Native.IMAGE, "width"), image.width());
            input.set(JAVA_LONG, offset(Native.IMAGE, "height"), image.height());
            input.set(JAVA_INT, offset(Native.IMAGE, "channels"), image.channels());
            input.set(JAVA_LONG, offset(Native.IMAGE, "stride"), image.stride());
            MemorySegment settings = arena.allocate(Native.SCAN_OPTIONS);
            settings.set(JAVA_INT, offset(Native.SCAN_OPTIONS, "formats"), scan.formats().map(Format::mask).orElse(0));
            MemorySegment out = arena.allocate(JAVA_LONG);
            MemorySegment error = arena.allocate(Native.ERROR);
            lib.check(Native.call(inspect ? lib.inspect : lib.scan, handle, input, settings, out, error), error);
            long result = out.get(JAVA_LONG, 0);
            try {
                return reader.apply(arena, result);
            } finally {
                lib.check(Native.call(lib.destroyResult, result));
            }
        }
    }

    private List<Barcode> readBarcodes(Arena arena, long result) {
        MemorySegment countOut = arena.allocate(JAVA_LONG);
        lib.check(Native.call(lib.count, result, countOut));
        long count = countOut.get(JAVA_LONG, 0);
        List<Barcode> barcodes = new ArrayList<>();
        MemorySegment value = arena.allocate(Native.BARCODE);
        for (long i = 0; i < count; i++) {
            lib.check(Native.call(lib.barcode, result, i, value));
            long appendCount = value.get(JAVA_LONG, offset(Native.BARCODE, "appendCount"));
            int parity = value.get(JAVA_INT, offset(Native.BARCODE, "parity"));
            Optional<StructuredAppend> append = appendCount == 0 ? Optional.empty()
                    : Optional.of(new StructuredAppend(value.get(JAVA_LONG, offset(Native.BARCODE, "appendIndex")), appendCount,
                            text(arena, result, i, STRUCTURED_APPEND_ID, value.get(JAVA_LONG, offset(Native.BARCODE, "appendIdLength"))),
                            parity < 0 ? OptionalInt.empty() : OptionalInt.of(parity)));
            barcodes.add(new Barcode(
                    text(arena, result, i, TEXT, value.get(JAVA_LONG, offset(Native.BARCODE, "textLength"))).orElseThrow(),
                    Format.fromBit(value.get(JAVA_INT, offset(Native.BARCODE, "format"))),
                    polygon(value, Native.BARCODE),
                    value.get(JAVA_LONG, offset(Native.BARCODE, "support")),
                    bytes(arena, result, i, PAYLOAD_BYTES, value.get(JAVA_LONG, offset(Native.BARCODE, "payloadLength"))),
                    text(arena, result, i, EAN_ADD_ON, value.get(JAVA_LONG, offset(Native.BARCODE, "addonLength"))),
                    tristate(value.get(JAVA_INT, offset(Native.BARCODE, "gs1"))),
                    tristate(value.get(JAVA_INT, offset(Native.BARCODE, "readerInitialization"))),
                    append));
        }
        return List.copyOf(barcodes);
    }

    private InspectionResult read(Arena arena, long result) {
        List<Barcode> barcodes = readBarcodes(arena, result);
        MemorySegment summary = arena.allocate(Native.SUMMARY);
        lib.check(Native.call(lib.info, result, summary));
        long regions = summary.get(JAVA_LONG, offset(Native.SUMMARY, "undecodedCount"));
        List<UndecodedRegion> undecoded = new ArrayList<>();
        MemorySegment region = arena.allocate(Native.REGION);
        for (long i = 0; i < regions; i++) {
            lib.check(Native.call(lib.undecoded, result, i, region));
            int format = region.get(JAVA_INT, offset(Native.REGION, "format"));
            undecoded.add(new UndecodedRegion(
                    format == 0 ? Optional.empty() : Optional.of(Format.fromBit(format)), polygon(region, Native.REGION)));
        }
        String json;
        {
            MemorySegment length = arena.allocate(JAVA_LONG);
            lib.check(Native.call(lib.jsonLength, result, length));
            long jsonLength = length.get(JAVA_LONG, 0);
            MemorySegment bytes = arena.allocate(jsonLength + 1);
            lib.check(Native.call(lib.copyJson, result, bytes, jsonLength + 1));
            json = new String(bytes.asSlice(0, jsonLength).toArray(JAVA_BYTE), StandardCharsets.UTF_8);
        }
        return new InspectionResult(barcodes, undecoded,
                Math.toIntExact(summary.get(JAVA_LONG, offset(Native.SUMMARY, "width"))), Math.toIntExact(summary.get(JAVA_LONG, offset(Native.SUMMARY, "height"))),
                Mode.fromCode(summary.get(JAVA_INT, offset(Native.SUMMARY, "mode"))), summary.get(JAVA_DOUBLE, offset(Native.SUMMARY, "elapsedMs")), json);
    }

    private Optional<byte[]> bytes(Arena arena, long result, long index, int field, long length) {
        if (length == Native.ABSENT) return Optional.empty();
        MemorySegment out = arena.allocate(length + 1);
        lib.check(Native.call(lib.copy, result, index, field, out, length + 1));
        return Optional.of(out.asSlice(0, length).toArray(JAVA_BYTE));
    }

    private Optional<String> text(Arena arena, long result, long index, int field, long length) {
        return bytes(arena, result, index, field, length).map(b -> new String(b, StandardCharsets.UTF_8));
    }

    private static List<Point> polygon(MemorySegment value, MemoryLayout layout) {
        long start = offset(layout, "polygon");
        long x = offset(Native.POINT, "x"), y = offset(Native.POINT, "y");
        List<Point> points = new ArrayList<>(4);
        for (long p = 0; p < 4; p++) {
            long point = start + p * Native.POINT.byteSize();
            points.add(new Point(value.get(JAVA_DOUBLE, point + x), value.get(JAVA_DOUBLE, point + y)));
        }
        return points;
    }

    private static Optional<Boolean> tristate(int value) {
        return value < 0 ? Optional.empty() : Optional.of(value != 0);
    }

    /** Release the native scanner; repeated calls are safe. */
    @Override
    public synchronized void close() {
        if (handle != 0) {
            long old = handle;
            handle = 0;
            lib.check(Native.call(lib.destroy, old));
        }
    }
}
