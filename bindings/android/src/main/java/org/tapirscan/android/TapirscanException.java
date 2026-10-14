package org.tapirscan.android;

/** A failed scanner call, with the library's message. */
public final class TapirscanException extends RuntimeException {
    private static final long serialVersionUID = 1L;
    private final int status;

    TapirscanException(int status, String message) {
        super(message);
        this.status = status;
    }

    /** The C status code. */
    public int status() {
        return status;
    }
}
