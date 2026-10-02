package org.tapirscan;

/** Whether to read the adjacent two/five-digit EAN/UPC supplement. */
public enum EanAddOnPolicy {
    /** Decode the main retail value without searching for a supplement (default). */
    IGNORE,
    /** Read a supplement when possible; keep the main value if none is readable. */
    READ,
    /** Return retail reads only with a readable supplement; other formats are unaffected. */
    REQUIRE
}
