package org.tapirscan.android;

/** Format masks; combine with {@code |}. Enable only the formats you need. */
public final class Formats {
    public static final int EAN13 = 1;
    public static final int UPCA = 2;
    public static final int EAN8 = 4;
    public static final int UPCE = 8;
    public static final int CODE128 = 16;
    public static final int CODE39 = 32;
    public static final int ITF = 64;
    public static final int CODABAR = 128;
    public static final int CODE93 = 256;
    public static final int QR_CODE = 512;
    public static final int DATA_MATRIX = 1024;
    public static final int PDF417 = 2048;
    public static final int AZTEC = 4096;
    public static final int DATA_BAR = 8192;
    public static final int DATA_BAR_EXPANDED = 16384;
    public static final int MAXI_CODE = 131072;
    /** EAN-13, UPC-A, EAN-8 and UPC-E (the default). */
    public static final int RETAIL = 15;
    /** Retail plus Code 128, Code 39 and ITF. */
    public static final int COMMON_1D = 127;
    /** Common linear formats plus QR Code and Data Matrix. */
    public static final int COMMON = 1663;
    public static final int ALL = 163839;

    private Formats() {}
}
