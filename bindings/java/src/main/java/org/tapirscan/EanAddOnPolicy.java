package org.tapirscan;

/** Whether to read the adjacent two/five-digit EAN/UPC supplement. */
public enum EanAddOnPolicy {
    /** Decode the main retail value without searching for a supplement (default). */
    IGNORE(0),
    /** Read a supplement when possible; keep the main value if none is readable. */
    READ(1),
    /** Return retail reads only with a readable supplement; other formats are unaffected. */
    REQUIRE(2);

    private final int code;

    EanAddOnPolicy(int code) {
        this.code = code;
    }

    /** Native {@code TAPIRSCAN_EAN_ADD_ON_*} value. */
    int code() {
        return code;
    }
}
