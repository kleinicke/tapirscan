package org.tapirscan.android;

import java.nio.ByteBuffer;
import java.util.Arrays;
import java.util.List;

/**
 * A reusable barcode scanner over the Tapirscan C library, through JNI. Create one
 * and reuse it for every frame; calls on one scanner run one at a time, and separate
 * scanners run concurrently. Close it when done.
 */
public final class BarcodeScanner implements AutoCloseable {
    public static final int MODE_LOW = 0;
    public static final int MODE_MEDIUM = 1;
    public static final int MODE_HIGH = 2;
    public static final int MODE_VERY_HIGH = 3;

    static {
        System.loadLibrary("tapirscan_jni");
    }

    private long handle;

    /** Medium effort and retail formats. */
    public BarcodeScanner() {
        this(MODE_MEDIUM, Formats.RETAIL);
    }

    /** {@code formats} is a {@link Formats} mask. */
    public BarcodeScanner(int mode, int formats) {
        handle = nativeCreate(mode, formats);
    }

    /**
     * Scan 8-bit gray (1 channel), RGB (3) or RGBA (4, alpha ignored) pixels. A direct
     * buffer, such as a CameraX {@code ImageProxy} plane, is read in place; pass
     * the Y plane with {@code channels = 1} and its row stride. {@code stride} is bytes
     * per row; 0 means packed rows. Pixels start at the buffer's position.
     */
    public synchronized List<Barcode> scan(
            ByteBuffer pixels, int width, int height, int channels, int stride) {
        if (pixels.isDirect()) {
            return Arrays.asList(nativeScanDirect(
                    open(), pixels, pixels.position(), pixels.remaining(),
                    width, height, channels, stride));
        }
        byte[] copy = new byte[pixels.remaining()];
        pixels.duplicate().get(copy);
        return scan(copy, width, height, channels, stride);
    }

    /** Scan pixels held in a byte array. */
    public synchronized List<Barcode> scan(
            byte[] pixels, int width, int height, int channels, int stride) {
        return Arrays.asList(nativeScanArray(open(), pixels, width, height, channels, stride));
    }

    @Override
    public synchronized void close() {
        if (handle != 0) {
            nativeDestroy(handle);
            handle = 0;
        }
    }

    private long open() {
        if (handle == 0) throw new IllegalStateException("Scanner is closed");
        return handle;
    }

    private static native long nativeCreate(int mode, int formats);

    private static native void nativeDestroy(long handle);

    private static native Barcode[] nativeScanDirect(long handle, ByteBuffer pixels, int offset,
            int length, int width, int height, int channels, int stride);

    private static native Barcode[] nativeScanArray(long handle, byte[] pixels, int width,
            int height, int channels, int stride);
}
