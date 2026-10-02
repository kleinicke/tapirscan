package org.tapirscan;

/** A native scanner failure; {@code code} is the C status. */
public final class ScannerException extends RuntimeException {
    private static final long serialVersionUID = 1L;
    /** Native status from tapirscan.h. */
    public final int code;

    ScannerException(int code, String message) {
        super(message + " (code " + code + ")");
        this.code = code;
    }
}
