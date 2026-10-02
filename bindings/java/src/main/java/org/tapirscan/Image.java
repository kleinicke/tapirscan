package org.tapirscan;

import java.lang.foreign.MemorySegment;
import java.util.Objects;

/**
 * Gray8, RGB8 or RGBA8 pixels (alpha ignored), at least 3x3 and at most 32 megapixels.
 * {@code stride} is bytes per row and 0 means packed rows. The addressed layout,
 * {@code (height - 1) * stride + width * channels} bytes, must fit in {@code pixels} and in
 * 128 MiB; a larger backing segment, such as a frame around a crop, is fine.
 *
 * <p>Array-backed pixels are copied when scanning. Native segments, for example from
 * {@code MemorySegment.ofBuffer(directBuffer)}, are read in place: keep them alive and
 * unchanged until the scan returns.
 */
public record Image(MemorySegment pixels, int width, int height, int channels, int stride) {
    private static final long MAX_LAYOUT_BYTES = 128L * 1024 * 1024;

    public Image {
        Objects.requireNonNull(pixels, "pixels");
        if (width < 0 || height < 0 || channels < 0 || stride < 0) {
            throw new IllegalArgumentException("image dimensions must not be negative");
        }
    }

    public static Image gray(byte[] data, int width, int height) {
        return gray(MemorySegment.ofArray(data), width, height);
    }

    public static Image gray(MemorySegment pixels, int width, int height) {
        return new Image(pixels, width, height, 1, 0);
    }

    public static Image rgb(byte[] data, int width, int height) {
        return rgb(MemorySegment.ofArray(data), width, height);
    }

    public static Image rgb(MemorySegment pixels, int width, int height) {
        return new Image(pixels, width, height, 3, 0);
    }

    public static Image rgba(byte[] data, int width, int height) {
        return rgba(MemorySegment.ofArray(data), width, height);
    }

    public static Image rgba(MemorySegment pixels, int width, int height) {
        return new Image(pixels, width, height, 4, 0);
    }

    /** The same pixels with explicit row padding. */
    public Image withStride(int bytes) {
        return new Image(pixels, width, height, channels, bytes);
    }

    /**
     * Bytes the layout addresses, limited to the available pixels and the layout limit so
     * oversized inputs are not copied before native validation rejects them.
     */
    long addressedBytes() {
        long row = (long) width * channels;
        long rowStride = stride == 0 ? row : stride;
        long addressed = Math.max(height - 1, 0) * rowStride + row;
        return Math.min(Math.min(pixels.byteSize(), addressed), MAX_LAYOUT_BYTES);
    }
}
