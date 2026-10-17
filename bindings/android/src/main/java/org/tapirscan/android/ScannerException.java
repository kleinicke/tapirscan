package org.tapirscan.android;

import java.nio.charset.StandardCharsets;

/** A native scanner failure; {@code code} is the C status. */
public final class ScannerException extends RuntimeException {
    private static final long serialVersionUID = 1L;
    /** Native status from tapirscan.h. */
    public final int code;

    ScannerException(int code, String message) {
        super(message + " (code " + code + ")");
        this.code = code;
    }

    /** Called from JNI with the UTF-8 message bytes. */
    ScannerException(int code, byte[] message) {
        this(code, new String(message, StandardCharsets.UTF_8));
    }

    /**
     * The failure name shared with the Python and JavaScript bindings: {@code "invalid_input"}
     * (status 1), {@code "disposed"} (2) or {@code "engine"} (3 and 4).
     */
    public String codeName() {
        return code == 1 ? "invalid_input" : code == 2 ? "disposed" : "engine";
    }
}
