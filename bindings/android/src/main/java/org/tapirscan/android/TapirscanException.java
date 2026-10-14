package org.tapirscan.android;

import java.nio.charset.StandardCharsets;

/** A failed scanner call, with the library's message. */
public final class TapirscanException extends RuntimeException {
    private static final long serialVersionUID = 1L;
    private final int status;

    TapirscanException(int status, String message) {
        super(message);
        this.status = status;
    }

    /** Called from JNI with the UTF-8 message bytes. */
    TapirscanException(int status, byte[] message) {
        this(status, new String(message, StandardCharsets.UTF_8));
    }

    /** The C status code. */
    public int status() {
        return status;
    }
}
