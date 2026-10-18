package org.tapirscan.android;

import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Objects;
import java.util.Optional;

/**
 * A reusable scanner over the Tapirscan C library, through JNI. Create one and reuse it
 * for every frame, then close it. Calls on one scanner run one at a time and separate
 * scanners run concurrently; results remain valid after {@link #close()}.
 *
 * <p>Pixels are 8-bit gray (1 channel), RGB (3) or RGBA (4, alpha ignored). A direct
 * buffer, such as a CameraX {@code ImageProxy} plane, is read in place from its position;
 * pass the Y plane with {@code channels = 1} and its row stride. {@code stride} is bytes
 * per row; 0 means packed rows.
 */
public final class Scanner implements AutoCloseable {
    static {
        System.loadLibrary("tapirscan_jni");
    }

    /** {@code tapirscan_parallel}: {@code DEFAULT} follows the process default. */
    private static final int PARALLEL_DEFAULT = 0, PARALLEL_OFF = 1, PARALLEL_ON = 2;

    private final ScannerOptions options;
    private long handle;

    /** Medium effort, Retail formats and ignored supplements. */
    public Scanner() {
        this(ScannerOptions.defaults());
    }

    public Scanner(ScannerOptions options) {
        this.options = Objects.requireNonNull(options);
        handle = nativeCreate(options.mode().code(), Format.mask(options.formats()),
                options.eanAddOnPolicy().code(), parallelCode(options.parallel()));
    }

    /** The native tapirscan_parallel value: the process default, off or on. */
    static int parallelCode(Boolean parallel) {
        return parallel == null ? PARALLEL_DEFAULT : parallel ? PARALLEL_ON : PARALLEL_OFF;
    }

    public ScannerOptions options() {
        return options;
    }

    public ScanResult scan(ByteBuffer pixels, int width, int height, int channels, int stride) {
        return scan(pixels, width, height, channels, stride, ScanOptions.defaults());
    }

    /** Scan one image. No detection is a successful empty result. */
    public synchronized ScanResult scan(ByteBuffer pixels, int width, int height, int channels,
            int stride, ScanOptions scan) {
        return new ScanResult(barcodes(run(pixels, width, height, channels, stride, scan, false)));
    }

    public ScanResult scan(byte[] pixels, int width, int height, int channels, int stride) {
        return scan(pixels, width, height, channels, stride, ScanOptions.defaults());
    }

    public synchronized ScanResult scan(byte[] pixels, int width, int height, int channels,
            int stride, ScanOptions scan) {
        return new ScanResult(barcodes(run(pixels, width, height, channels, stride, scan, false)));
    }

    public InspectionResult inspect(ByteBuffer pixels, int width, int height, int channels, int stride) {
        return inspect(pixels, width, height, channels, stride, ScanOptions.defaults());
    }

    /** Inspect one image, including unread regions and diagnostics. */
    public synchronized InspectionResult inspect(ByteBuffer pixels, int width, int height,
            int channels, int stride, ScanOptions scan) {
        return inspection(run(pixels, width, height, channels, stride, scan, true));
    }

    public InspectionResult inspect(byte[] pixels, int width, int height, int channels, int stride) {
        return inspect(pixels, width, height, channels, stride, ScanOptions.defaults());
    }

    public synchronized InspectionResult inspect(byte[] pixels, int width, int height, int channels,
            int stride, ScanOptions scan) {
        return inspection(run(pixels, width, height, channels, stride, scan, true));
    }

    @Override
    public synchronized void close() {
        if (handle != 0) {
            nativeDestroy(handle);
            handle = 0;
        }
    }

    private Object[] run(ByteBuffer pixels, int width, int height, int channels, int stride,
            ScanOptions scan, boolean inspect) {
        Objects.requireNonNull(pixels, "pixels");
        long open = open();
        int formats = formats(scan);
        if (pixels.isDirect()) {
            return nativeScanDirect(open, pixels, pixels.position(), pixels.remaining(), width,
                    height, channels, stride, formats, inspect);
        }
        byte[] copy = new byte[pixels.remaining()];
        pixels.duplicate().get(copy);
        return nativeScanArray(open, copy, width, height, channels, stride, formats, inspect);
    }

    private Object[] run(byte[] pixels, int width, int height, int channels, int stride,
            ScanOptions scan, boolean inspect) {
        Objects.requireNonNull(pixels, "pixels");
        return nativeScanArray(open(), pixels, width, height, channels, stride, formats(scan), inspect);
    }

    private long open() {
        if (handle == 0) throw new IllegalStateException("Scanner is closed");
        return handle;
    }

    /** The per-call format mask; 0 selects the scanner's formats. */
    private static int formats(ScanOptions scan) {
        Objects.requireNonNull(scan);
        return scan.formats().map(Format::mask).orElse(0);
    }

    private static List<Barcode> barcodes(Object[] raw) {
        return Arrays.asList((Barcode[]) raw[0]);
    }

    private static InspectionResult inspection(Object[] raw) {
        double[] regions = (double[]) raw[1];
        List<UndecodedRegion> undecoded = new ArrayList<>();
        for (int r = 0; r + 9 <= regions.length; r += 9) {
            List<Point> polygon = new ArrayList<>(4);
            for (int p = 0; p < 4; p++) polygon.add(new Point(regions[r + 2 * p], regions[r + 2 * p + 1]));
            int format = (int) regions[r + 8];
            undecoded.add(new UndecodedRegion(
                    format == 0 ? Optional.empty() : Optional.of(Format.fromBit(format)), polygon));
        }
        double[] summary = (double[]) raw[2];
        return new InspectionResult(barcodes(raw), undecoded, (int) summary[0], (int) summary[1],
                Mode.fromCode((int) summary[2]), summary[3],
                new String((byte[]) raw[3], StandardCharsets.UTF_8));
    }

    private static native long nativeCreate(int mode, int formats, int addOnPolicy, int parallel);

    private static native void nativeDestroy(long handle);

    /**
     * Returns {Barcode[]}, or for an inspection {Barcode[], double[] regions (eight polygon
     * coordinates and the format bit per region), double[] {width, height, mode, elapsedMs},
     * byte[] diagnostics JSON}.
     */
    private static native Object[] nativeScanDirect(long handle, ByteBuffer pixels, int offset,
            int length, int width, int height, int channels, int stride, int formats,
            boolean inspect);

    private static native Object[] nativeScanArray(long handle, byte[] pixels, int width,
            int height, int channels, int stride, int formats, boolean inspect);
}
