package org.tapirscan;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_DOUBLE;
import static java.lang.foreign.MemoryLayout.PathElement.groupElement;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandle;
import java.nio.file.Path;

/** Native ABI 6 entry points from tapirscan.h, loaded once per JVM. */
final class Native {
    static final int ABI_VERSION = 6;
    static final MemoryLayout IMAGE = MemoryLayout.structLayout(
            ADDRESS.withName("data"), JAVA_LONG.withName("length"),
            JAVA_LONG.withName("width"), JAVA_LONG.withName("height"),
            JAVA_INT.withName("channels"), MemoryLayout.paddingLayout(4), JAVA_LONG.withName("stride"));
    static final MemoryLayout SUMMARY = MemoryLayout.structLayout(
            JAVA_LONG.withName("barcodeCount"), JAVA_LONG.withName("undecodedCount"),
            JAVA_LONG.withName("width"), JAVA_LONG.withName("height"),
            JAVA_DOUBLE.withName("elapsedMs"), JAVA_INT.withName("mode"), MemoryLayout.paddingLayout(4));
    static final MemoryLayout POINT = MemoryLayout.structLayout(JAVA_DOUBLE.withName("x"), JAVA_DOUBLE.withName("y"));
    static final MemoryLayout POLYGON = MemoryLayout.sequenceLayout(4, POINT);
    static final MemoryLayout BARCODE = MemoryLayout.structLayout(
            POLYGON.withName("polygon"), JAVA_LONG.withName("support"), JAVA_INT.withName("format"),
            JAVA_INT.withName("gs1"), JAVA_INT.withName("readerInitialization"), JAVA_INT.withName("parity"),
            JAVA_LONG.withName("appendIndex"), JAVA_LONG.withName("appendCount"),
            JAVA_LONG.withName("textLength"), JAVA_LONG.withName("payloadLength"),
            JAVA_LONG.withName("addonLength"), JAVA_LONG.withName("appendIdLength"));
    static final MemoryLayout REGION = MemoryLayout.structLayout(
            POLYGON.withName("polygon"), JAVA_INT.withName("format"), MemoryLayout.paddingLayout(4));
    static final MemoryLayout SCANNER_OPTIONS = MemoryLayout.structLayout(
            JAVA_INT.withName("mode"), JAVA_INT.withName("formats"), JAVA_INT.withName("addonPolicy"));
    static final MemoryLayout SCAN_OPTIONS = MemoryLayout.structLayout(
            JAVA_INT.withName("formats"), JAVA_INT.withName("extendedBudget"));
    static final MemoryLayout ERROR = MemoryLayout.sequenceLayout(512, JAVA_BYTE);

    static long offset(MemoryLayout layout, String field) {
        return layout.byteOffset(groupElement(field));
    }
    static final long ABSENT = -1L;

    private static Native instance;

    final MethodHandle statusMessage, create, destroy, scan, inspect, count, info, barcode, undecoded, copy,
            copyJson, jsonLength, destroyResult;

    static synchronized Native get() {
        if (instance == null) instance = new Native();
        return instance;
    }

    // Loading the native scanner library is this binding's purpose.
    @SuppressWarnings("restricted")
    private Native() {
        String file = System.getProperty("tapirscan.library");
        String directory = System.getenv("TAPIRSCAN_LIBRARY_DIR");
        String name = System.mapLibraryName("tapirscan");
        SymbolLookup symbols = file != null
                ? SymbolLookup.libraryLookup(Path.of(file), Arena.global())
                : directory != null
                        ? SymbolLookup.libraryLookup(Path.of(directory).resolve(name), Arena.global())
                        : SymbolLookup.libraryLookup(name, Arena.global());
        int abi = (int) call(bind(symbols, "tapirscan_abi_version", FunctionDescriptor.of(JAVA_INT)));
        if (abi != ABI_VERSION) {
            throw new IllegalStateException("Native ABI mismatch: expected " + ABI_VERSION + ", got "
                    + abi + ". Rebuild the native library.");
        }
        statusMessage = bind(symbols, "tapirscan_status_message", FunctionDescriptor.of(ADDRESS, JAVA_INT));
        create = bind(symbols, "tapirscan_scanner_create", FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS, ADDRESS));
        destroy = bind(symbols, "tapirscan_scanner_destroy", FunctionDescriptor.of(JAVA_INT, JAVA_LONG));
        scan = bind(symbols, "tapirscan_scan", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, ADDRESS, ADDRESS, ADDRESS, ADDRESS));
        inspect = bind(symbols, "tapirscan_inspect", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, ADDRESS, ADDRESS, ADDRESS, ADDRESS));
        count = bind(symbols, "tapirscan_result_count", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, ADDRESS));
        info = bind(symbols, "tapirscan_result_info", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, ADDRESS));
        barcode = bind(symbols, "tapirscan_result_barcode", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, JAVA_LONG, ADDRESS));
        undecoded = bind(symbols, "tapirscan_result_undecoded", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, JAVA_LONG, ADDRESS));
        copy = bind(symbols, "tapirscan_result_copy",
                FunctionDescriptor.of(JAVA_INT, JAVA_LONG, JAVA_LONG, JAVA_INT, ADDRESS, JAVA_LONG));
        jsonLength = bind(symbols, "tapirscan_result_json_length", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, ADDRESS));
        copyJson = bind(symbols, "tapirscan_result_copy_json", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, ADDRESS, JAVA_LONG));
        destroyResult = bind(symbols, "tapirscan_result_destroy", FunctionDescriptor.of(JAVA_INT, JAVA_LONG));
    }

    // The descriptors match the version-checked C declarations.
    @SuppressWarnings("restricted")
    private static MethodHandle bind(SymbolLookup symbols, String name, FunctionDescriptor descriptor) {
        MemorySegment symbol = symbols.find(name)
                .orElseThrow(() -> new IllegalStateException("Native library lacks " + name));
        return Linker.nativeLinker().downcallHandle(symbol, descriptor);
    }

    static Object call(MethodHandle method, Object... args) {
        try {
            return method.invokeWithArguments(args);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("Native call failed", e);
        }
    }

    // Status messages are static NUL-terminated C strings.
    @SuppressWarnings("restricted")
    void check(Object status) {
        check(status, MemorySegment.NULL);
    }

    @SuppressWarnings("restricted")
    void check(Object status, MemorySegment error) {
        int code = (int) status;
        if (code != 0) {
            if (!error.equals(MemorySegment.NULL) && error.get(JAVA_BYTE, 0) != 0) {
                throw new ScannerException(code, error.getString(0));
            }
            MemorySegment message = (MemorySegment) call(statusMessage, code);
            throw new ScannerException(code, message.reinterpret(Long.MAX_VALUE).getString(0));
        }
    }
}
