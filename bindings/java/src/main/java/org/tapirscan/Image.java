package org.tapirscan;

import java.util.Objects;

/**
 * Gray8, RGB8 or RGBA8 pixels (alpha ignored), at least 3x3 and at most 128 MiB.
 * The array is copied when scanning; {@code stride} is bytes per row and 0 means packed.
 */
public record Image(byte[] data, int width, int height, int channels, int stride) {
    public Image {
        Objects.requireNonNull(data, "data");
    }

    public static Image gray(byte[] data, int width, int height) {
        return new Image(data, width, height, 1, 0);
    }

    public static Image rgb(byte[] data, int width, int height) {
        return new Image(data, width, height, 3, 0);
    }

    public static Image rgba(byte[] data, int width, int height) {
        return new Image(data, width, height, 4, 0);
    }

    /** The same pixels with explicit row padding. */
    public Image withStride(int bytes) {
        return new Image(data, width, height, channels, bytes);
    }
}
