# Fast end-to-end scanning

Most of a scan's cost is decided before the scanner runs: which formats you enable,
which effort mode you choose, how large the image is, and how you hand the pixels
over. This guide lists the choices that matter, with measured costs, and the fastest
input path for each environment.

## The five rules

1. **Enable only the formats you need.** The default is retail EAN/UPC. Every
   enabled 2D reader (QR Code, Data Matrix, Aztec, PDF417, MaxiCode) adds its own
   search of the whole image. Use an exact list such as `["EAN13", "QRCode"]`, or
   `retail` or `common1D`. Keep `all` for exploration.
2. **Create one scanner and reuse it.** The one-shot `scan()` helpers create and
   dispose a scanner on every call; in the browser that also starts a worker.
3. **Pick the mode for the job.** `low` for live camera frames, `medium` for
   photos, `high` and `very-high` only when an image needs it.
4. **Scan the size you need.** Camera frames of 1-2 megapixels (720p, 1080p) are
   much cheaper than 12-megapixel stills, especially with 2D formats enabled.
   Images above 32 megapixels are reduced internally, but still cost decoding,
   memory and copies. When the barcodes are large in a photo, decode it at reduced size, as with
   Pillow's `Image.draft` for JPEGs: a 12-megapixel photo with all formats scans
   in 53 ms at half size instead of 231 ms (Apple M1 Pro).
5. **Hand over decoded 8-bit pixels as you have them.** Packed RGBA is the cheapest
   input for the default retail formats; gray and RGB inputs are copied to RGBA
   internally (about 1.5-3 ms at 12 megapixels), so do not convert RGBA to gray
   for scanning. If you must convert anyway, for example from OpenCV's BGR, convert
   to RGBA. Skip encode/decode
   round trips, such as drawing to a canvas or saving a JPEG, when you already hold
   pixels.

## What it costs

Per-scan times on 960×960 photo crops with mixed formats, native library, one scan at a time. With several 2D formats enabled,
native builds run those readers on separate threads ("parallel", the default; see
[threads](#threads-and-servers)):

| Mode and formats    | M1 Pro, one thread | M1 Pro, parallel | Ryzen 7 5800X, one thread | Ryzen, parallel |
| ------------------- | -----------------: | ---------------: | ------------------------: | --------------: |
| Low, retail         |             4.9 ms |           4.9 ms |                    5.2 ms |          5.3 ms |
| Medium, retail      |              33 ms |            33 ms |                     37 ms |           37 ms |
| Low, all formats    |              33 ms |            17 ms |                     36 ms |           21 ms |
| Medium, all formats |              54 ms |            35 ms |                     58 ms |           43 ms |

WebAssembly (browser and Node) runs about 1.5× slower than native and always on
one thread.

Input handling on top of the scan, for a 12-megapixel RGBA image (Linux makes
fresh memory more expensive, hence the larger x86 numbers):

| Input                                               | M1 Pro (macOS) | Ryzen (Linux) |
| --------------------------------------------------- | -------------: | ------------: |
| C, C++, Rust, Java `MemorySegment`: borrowed buffer |           0 ms |          0 ms |
| Python uint8 NumPy array                            |           1 ms |          4 ms |
| Java `byte[]` (copied into a reused buffer)         |           1 ms |          5 ms |
| Browser decoded pixels with `{ transfer: true }`    |          <1 ms |         <1 ms |
| Browser decoded pixels or `ImageData`               |           5 ms |         27 ms |
| Browser `<canvas>` or `<video>` source              |          35 ms |         36 ms |
| Browser JPEG `Blob`/`File` (includes JPEG decoding) |          65 ms |         98 ms |

## Browser

- **Reuse one `Scanner`** from `tapirscan/browser` and `dispose()` it when the
  view closes. It runs in its own worker, so scans never block the page.
- **Camera loops:** request a moderate resolution, use Low (or the experimental
  Turbo presets for 1D), select only your formats, and await each scan before
  starting the next:

  ```js
  const stream = await navigator.mediaDevices.getUserMedia({
    video: { facingMode: "environment", width: { ideal: 1280 }, height: { ideal: 720 } },
  });
  video.srcObject = stream;
  await video.play();
  const scanner = new Scanner({ mode: "low", formats: ["EAN13", "QRCode"] });
  while (running) {
    await new Promise((resolve) => video.requestVideoFrameCallback(resolve));
    const result = await scanner.scan(video);
    // ...
  }
  ```

- **Pixels you already have:** pass `ImageData` or `{ data, width, height, channels }`.
  `scanner.scan(pixels, { transfer: true })` hands the buffer to the worker instead
  of copying it; the buffer is then detached.
- **Files:** pass the `File`/`Blob` directly; the worker decodes it. Large photos are
  decoded in bands, without a full-size canvas.
- **Memory:** a scanner's WASM memory grows to its largest image and is released by
  `dispose()`. On phones, scan occasional large photos with a separate scanner and
  dispose it; keep one long-lived scanner for the camera.
- **Frameworks:** create the scanner when the component mounts and dispose it when
  it unmounts (React `useEffect`, Vue `onMounted`/`onBeforeUnmount`, Svelte
  `onMount`/`onDestroy`). The [JavaScript guide](../bindings/javascript/README.md)
  has React, Svelte, Vite and Next.js examples.

## Node

Use `await Scanner.create()` once and `scanner.scan()` per image; the one-shot
`scan()` costs about 2 ms extra per call. Pass the decoded buffer as it comes, for
example `sharp(file).raw()` with `channels: info.channels`.

## Python

- **Inputs, cheapest first:** contiguous uint8 NumPy arrays (RGBA, then RGB or gray);
  uint8 tensors; float arrays and tensors (validated and rounded, multithreaded for
  large arrays); Pillow images (one `tobytes()` copy). CUDA tensors are converted
  on the GPU and transferred once, so scan tensors where they already are. Moving
  a CPU tensor to the GPU only for scanning does not help uint8 tensors; for a
  large float tensor it saves about 9 ms at 12 MP (Ryzen 7 5800X, RTX 5060 Ti),
  because the upload costs almost as much as the CPU conversion.
- **OpenCV** images are BGR. `cv2.cvtColor(frame, cv2.COLOR_BGR2RGBA)` is the
  fastest way to scan them (12 MP: about 15 ms in total on an M1, against 32 ms
  with `color_order="BGR"`, which reorders the channels in Python).
- **Batches:** `tapirscan.scan_many(images, formats=...)` scans on all CPU cores,
  one scanner per thread, with results in input order.
- **Threads and servers:** scanning releases the GIL. Use one `Scanner` per
  thread (for example `threading.local()` or one per worker in a
  `ThreadPoolExecutor`); throughput scales with cores. In async frameworks such as
  FastAPI, run scans in a thread pool or a sync endpoint so they do not block the
  event loop. With `multiprocessing`, create the scanner inside each process.

## Java and Kotlin (JVM)

Pass a native `MemorySegment` (for example `MemorySegment.ofBuffer(directBuffer)`)
to scan without a copy; `byte[]` inputs are copied into a buffer the scanner reuses
until `close()`. Use one `Scanner` per thread. Android is not supported by the Java
binding (it needs the Foreign Function & Memory API); see [mobile apps](#mobile-apps).

## C, C++ and Rust

- Images are borrowed, never copied: pass your buffer with its row stride.
- **OpenCV `cv::Mat`:** there is no BGR layout; convert to RGBA, the cheapest input:

  ```cpp
  cv::Mat rgba;
  cv::cvtColor(frame, rgba, cv::COLOR_BGR2RGBA);
  auto image = tapirscan::Image::rgba(rgba.data, rgba.step * rgba.rows, rgba.cols, rgba.rows)
                   .with_stride(rgba.step);
  auto result = scanner.scan(image);
  ```

- **Rust on x86-64:** build with `RUSTFLAGS="-C target-cpu=x86-64-v2"` or in
  `.cargo/config.toml` (`rustflags = ["-C", "target-cpu=x86-64-v2"]`); scans are
  20-30% faster. The prebuilt native library already uses it. With the `image`
  crate, `GrayImage`, `RgbImage`, `RgbaImage` and 8-bit `DynamicImage`s are
  borrowed without copying.

## Threads and servers

A scanner serves one call at a time; separate scanners run concurrently, so use one
per thread. Native builds additionally run one scan's 1D formats and enabled 2D
readers on separate threads, which shortens a single scan with all formats by
1.35-1.9× on 960×960 photos and up to 2.9× on 12-megapixel images. On
servers that already run one scanner per core, create the scanner with the
`parallel` option off (`parallel=False` in Python, `ScannerOptions { parallel:
false, ..ScannerOptions::default() }` in Rust, `withParallel(false)` in Java and Android, `parallel = false`
in C++, `parallel: false` in Swift, `TAPIRSCAN_PARALLEL_OFF` in C); this saves the few percent of extra CPU time that
the parallel readers cost. Results are identical either way. Without the option,
`TAPIRSCAN_THREADS=0` or `1` in the process environment is the default for every
scanner; it is read once, at the first use. WebAssembly scans always use one
thread and ignore the option.

## Mobile apps

- **Web apps and WebViews:** use `tapirscan/browser`, as above.
- **iOS:** the [Swift package](../bindings/swift/README.md) scans AVFoundation frames
  through their luma plane, in place, and `CGImage`s.
- **Android:** the [Android AAR](../bindings/android/README.md) scans the Y plane of
  CameraX frames in place through JNI (64-bit ABIs). 32-bit devices can use the
  browser build in a WebView.
- On phones, use Low, 720p-1080p frames and only the formats you need; scanning the
  camera's luma plane avoids both a copy and a color conversion.
