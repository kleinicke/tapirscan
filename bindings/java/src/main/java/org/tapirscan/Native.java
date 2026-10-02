package org.tapirscan;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandle;
import java.nio.file.Path;

/** Native ABI 5 entry points from tapirscan.h, loaded once per JVM. */
final class Native {
    static final int ABI_VERSION = 5;
    // Struct sizes from tapirscan.h; offsets are documented where they are read.
    static final long IMAGE = 48, SUMMARY = 64, BARCODE = 136, REGION = 72, OPTIONS = 12;
    static final long ABSENT = -1L;

    private static Native instance;

    final MethodHandle statusMessage, create, destroy, scan, info, barcode, undecoded, copy,
            copyJson, destroyResult;

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
        create = bind(symbols, "tapirscan_scanner_create", FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS));
        destroy = bind(symbols, "tapirscan_scanner_destroy", FunctionDescriptor.of(JAVA_INT, JAVA_LONG));
        scan = bind(symbols, "tapirscan_scan", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, ADDRESS, ADDRESS, ADDRESS));
        info = bind(symbols, "tapirscan_result_info", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, ADDRESS));
        barcode = bind(symbols, "tapirscan_result_barcode", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, JAVA_LONG, ADDRESS));
        undecoded = bind(symbols, "tapirscan_result_undecoded", FunctionDescriptor.of(JAVA_INT, JAVA_LONG, JAVA_LONG, ADDRESS));
        copy = bind(symbols, "tapirscan_result_copy",
                FunctionDescriptor.of(JAVA_INT, JAVA_LONG, JAVA_LONG, JAVA_INT, ADDRESS, JAVA_LONG));
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
        int code = (int) status;
        if (code != 0) {
            MemorySegment message = (MemorySegment) call(statusMessage, code);
            throw new ScannerException(code, message.reinterpret(Long.MAX_VALUE).getString(0));
        }
    }
}
