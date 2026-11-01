# Scan sessions (preview)

Scan sessions turn decoded results into an intentional single scan or a list.
They own no scanner, camera, storage or UI. This is an additive preview API;
minor releases may refine it. The native ABI and decoder are unchanged.

## Shared behavior

- Single mode (the default) finishes after the first accepted identity. List mode finishes explicitly.
- Identity is the exact format and decoded text, including empty text, whitespace and Unicode encoding differences. No trimming, case folding or Unicode normalization is applied. Different formats remain separate.
- A repeated identity is ignored for the entire session. Explicit repeat acceptance adds one unit. Never enable repeat for every camera frame: quantities count accepted units, not distinct physical objects.
- Entries retain first-acceptance order. Undo reverses the last successful acceptance, reducing its quantity or removing it. Ignored duplicates do not add undo steps.
- Finish locks acceptance and undo, including for an empty list. Reset empties the session, clears history and retains the selected mode.
- Entries and finished results are immutable snapshots unaffected by later edits or reset.
- Keep each session on one thread, actor or isolate. History grows with accepted units; reset or finish bounded application tasks instead of keeping an unlimited session alive.
- Quantities have the JavaScript safe-integer ceiling (9,007,199,254,740,991). Overflow is an error (a precondition failure in Swift).

The application decides which decoded results to accept. When several barcodes
are visible, ask the user to select one or explicitly choose list behavior.
Temporal confirmation, camera stopping, focus, permissions, feedback, persistence
and JSON serialization remain the application's responsibility.

## Language coverage

| Binding                 | Session creation                                                                  | Accept a decoded barcode                       |
| ----------------------- | --------------------------------------------------------------------------------- | ---------------------------------------------- |
| JavaScript / TypeScript | `new ScanSession({mode: "list"})` from `tapirscan/session`                        | `session.accept(barcode)`                      |
| Python                  | `ScanSession(mode="list")` from `tapirscan`                                       | `session.accept(barcode.format, barcode.text)` |
| Java                    | `new ScanSession(ScanSession.Mode.LIST)` from `org.tapirscan`                     | `session.accept(barcode)`                      |
| Android Java / Kotlin   | Same Java API, package `org.tapirscan.android`                                    | `session.accept(barcode)`                      |
| Swift                   | `ScanSession(mode: .list)` from `Tapirscan`                                       | `session.accept(barcode)`                      |
| Dart / Flutter          | `ScanSession(mode: ScanSessionMode.list)` from `package:tapirscan/tapirscan.dart` | `session.acceptBarcode(barcode)`               |

Java and Android also accept `(Format, String)` directly. Swift accepts
`(format: Format, text: String)`; Dart accepts `(Format, String)`. Python names
the format argument `barcode_format`. To add an intentional extra unit, pass
`repeat=True` in Python, `repeat: true` in JavaScript/Swift/Dart, or a third `true`
argument to Java's format/text overload.

All expose `entries`, `finished`, `undo`, `finish` and `reset` (Java uses methods
for its accessors). The undo availability property is `can_undo` in Python,
`canUndo` in JavaScript/Swift/Dart, and `canUndo()` in Java/Android.
C, C++ and Rust remain decoding APIs without a session helper.

## Camera controls are separate

The JavaScript camera controller's focus, zoom, torch, confirmation and recovery
features work in supported browsers subject to their camera capabilities. These
are not native camera wrappers. Native apps continue to own their capture layer;
this session helper can consume results from that layer without controlling it.

The browser workflow example includes its own photo fallback, selection UI and
JSON export. Those UI features are not automatically provided by native sessions.
