# Shared-retail reader adapter

This crate root is the explicit build boundary used by shared-retail WASM modes.
It owns the small data types and `recovery_gray` entry point needed by the core.
The reader algorithms remain in the hash-pinned `multiformat/src` snapshot and
are copied into the prepared crate only after their hashes are verified.

`source.json` pins both sides of the boundary. The adapter crate hashes equal the
bytes produced by the former marker-based build adapter, so changing build
architecture does not change the compiled Rust input. Update these hashes only
with corresponding artifact and behavior evidence.
