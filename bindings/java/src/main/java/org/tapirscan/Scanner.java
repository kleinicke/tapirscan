package org.tapirscan;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_DOUBLE;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
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
    private static final long MAX_BYTES = 128L * 1024 * 1024;
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
            MemorySegment config = arena.allocate(Native.OPTIONS, 4);
            config.set(JAVA_INT, 0, options.mode().ordinal());
            config.set(JAVA_INT, 4, Format.mask(options.formats()));
            config.set(JAVA_INT, 8, options.eanAddOnPolicy().ordinal());
            MemorySegment out = arena.allocate(JAVA_LONG);
            lib.check(Native.call(lib.create, config, out));
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
    public synchronized ScanResult scan(Image image, ScanOptions scan) {
        Objects.requireNonNull(image);
        Objects.requireNonNull(scan);
        if (handle == 0) throw new IllegalStateException("Scanner is closed");
        int length = Math.toIntExact(Math.min(image.data().length, MAX_BYTES));
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment pixels = arena.allocate(Math.max(length, 1));
            pixels.copyFrom(MemorySegment.ofArray(image.data()).asSlice(0, length));
            MemorySegment input = arena.allocate(Native.IMAGE, 8);
            input.set(ADDRESS, 0, pixels);
            input.set(JAVA_LONG, 8, length);
            input.set(JAVA_LONG, 16, image.width());
            input.set(JAVA_LONG, 24, image.height());
            input.set(JAVA_INT, 32, image.channels());
            input.set(JAVA_LONG, 40, image.stride());
            MemorySegment settings = arena.allocate(Native.OPTIONS, 4);
            settings.set(JAVA_INT, 0, scan.formats().map(Format::mask).orElse(0));
            settings.set(JAVA_INT, 4, scan.debug() ? 1 : 0);
            settings.set(JAVA_INT, 8, scan.extendedBudget() ? 1 : 0);
            MemorySegment out = arena.allocate(JAVA_LONG);
            lib.check(Native.call(lib.scan, handle, input, settings, out));
            long result = out.get(JAVA_LONG, 0);
            try {
                return read(arena, result, scan.debug());
            } finally {
                lib.check(Native.call(lib.destroyResult, result));
            }
        }
    }

    private ScanResult read(Arena arena, long result, boolean debug) {
        MemorySegment summary = arena.allocate(Native.SUMMARY, 8);
        lib.check(Native.call(lib.info, result, summary));
        long count = summary.get(JAVA_LONG, 0), regions = summary.get(JAVA_LONG, 8);
        long best = summary.get(JAVA_LONG, 16), jsonLength = summary.get(JAVA_LONG, 24);
        List<Barcode> barcodes = new ArrayList<>();
        MemorySegment value = arena.allocate(Native.BARCODE, 8);
        for (long i = 0; i < count; i++) {
            lib.check(Native.call(lib.barcode, result, i, value));
            long appendCount = value.get(JAVA_LONG, 96);
            int parity = value.get(JAVA_INT, 84);
            Optional<StructuredAppend> append = appendCount == 0 ? Optional.empty()
                    : Optional.of(new StructuredAppend(value.get(JAVA_LONG, 88), appendCount,
                            text(arena, result, i, STRUCTURED_APPEND_ID, value.get(JAVA_LONG, 128)),
                            parity < 0 ? OptionalInt.empty() : OptionalInt.of(parity)));
            barcodes.add(new Barcode(
                    text(arena, result, i, TEXT, value.get(JAVA_LONG, 104)).orElseThrow(),
                    Format.fromBit(value.get(JAVA_INT, 72)),
                    polygon(value),
                    value.get(JAVA_LONG, 64),
                    bytes(arena, result, i, PAYLOAD_BYTES, value.get(JAVA_LONG, 112)),
                    text(arena, result, i, EAN_ADD_ON, value.get(JAVA_LONG, 120)),
                    tristate(value.get(JAVA_INT, 76)),
                    tristate(value.get(JAVA_INT, 80)),
                    append));
        }
        List<UndecodedRegion> undecoded = new ArrayList<>();
        MemorySegment region = arena.allocate(Native.REGION, 8);
        for (long i = 0; i < regions; i++) {
            lib.check(Native.call(lib.undecoded, result, i, region));
            int format = region.get(JAVA_INT, 64);
            undecoded.add(new UndecodedRegion(
                    format == 0 ? Optional.empty() : Optional.of(Format.fromBit(format)), polygon(region)));
        }
        Optional<String> json = Optional.empty();
        if (debug) {
            MemorySegment bytes = arena.allocate(jsonLength + 1);
            lib.check(Native.call(lib.copyJson, result, bytes, jsonLength + 1));
            json = Optional.of(new String(bytes.asSlice(0, jsonLength).toArray(JAVA_BYTE), StandardCharsets.UTF_8));
        }
        return new ScanResult(barcodes, undecoded,
                Math.toIntExact(summary.get(JAVA_LONG, 32)), Math.toIntExact(summary.get(JAVA_LONG, 40)),
                Mode.values()[summary.get(JAVA_INT, 56)], summary.get(JAVA_DOUBLE, 48),
                summary.get(JAVA_INT, 60) != 0, json,
                best < 0 ? OptionalInt.empty() : OptionalInt.of(Math.toIntExact(best)));
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

    private static List<Point> polygon(MemorySegment value) {
        List<Point> points = new ArrayList<>(4);
        for (long p = 0; p < 4; p++) {
            points.add(new Point(value.get(JAVA_DOUBLE, p * 16), value.get(JAVA_DOUBLE, p * 16 + 8)));
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
