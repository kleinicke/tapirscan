# Fast end-to-end scanning

Most of a scan's cost is decided before the scanner runs: which formats you enable,
which effort mode you choose, how large the image is, and how you hand the pixels
over. This guide describes the input paths and how to avoid unnecessary copies.

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
   Pillow's `Image.draft` for JPEGs. Check that the reduction preserves the smallest
   symbols you need to read.
5. **Hand over decoded 8-bit pixels as you have them.** Packed RGBA is the cheapest
   input for the default retail formats; gray and RGB inputs are copied to RGBA
   internally, so do not convert RGBA to gray
   for scanning. If you must convert anyway, for example from OpenCV's BGR, convert
   to RGBA. Skip encode/decode
   round trips, such as drawing to a canvas or saving a JPEG, when you already hold
   pixels.

## What it costs

Input handling adds work before the scan:

| Input                                            | Input handling                                         |
| ------------------------------------------------ | ------------------------------------------------------ |
| C, C++, Rust, Java `MemorySegment`               | Borrows the supplied buffer                            |
| Python uint8 NumPy array                         | Copies a snapshot                                      |
| Java `byte[]`                                    | Copies into a reused buffer                            |
| Browser decoded pixels with `{ transfer: true }` | Transfers a whole buffer; partial views require a copy |
| Browser decoded pixels or `ImageData`            | Copies pixels to the worker                            |
| Browser `<canvas>` or `<video>`                  | Extracts pixels and sends them to the worker           |
| Browser JPEG `Blob`/`File`                       | Decodes the image before scanning                      |

WebAssembly runs on one thread. Native builds can run format readers in parallel;
measure both single-scan latency and batch throughput for your workload.

Historical [photo-crop timings](https://github.com/kleinicke/tapirscan/commit/9205700)
and the [x86-64 comparison](https://github.com/kleinicke/tapirscan/commit/637828d)
are recorded in development commits. They lack a complete reproducible report
and do not establish speed ratios for the final release. New comparisons should
follow the [evaluation protocol](BENCHMARKS.md).

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
`scan()` also pays initialization cost on each call. Pass the decoded buffer as it comes, for
example `sharp(file).raw()` with `channels: info.channels`.

## Python

- **Inputs, cheapest first:** contiguous uint8 NumPy arrays (RGBA, then RGB or gray);
  uint8 tensors; float arrays and tensors (validated and rounded, multithreaded for
  large arrays); Pillow images (one `tobytes()` copy). CUDA tensors are converted
  on the GPU and transferred once, so scan tensors where they already are. Include
  upload and download costs when comparing CPU and GPU conversion.
- **OpenCV** images are BGR. `cv2.cvtColor(frame, cv2.COLOR_BGR2RGBA)`
  avoids reordering through the Python `color_order="BGR"` adapter.
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
  `.cargo/config.toml` (`rustflags = ["-C", "target-cpu=x86-64-v2"]`) to
  use the same CPU baseline as the prebuilt native library. With the `image`
  crate, `GrayImage`, `RgbImage`, `RgbaImage` and 8-bit `DynamicImage`s are
  borrowed without copying.

## Threads and servers

A scanner serves one call at a time; separate scanners run concurrently, so use one
per thread. Native builds additionally run one scan's 1D formats and enabled 2D
readers on separate threads. On
servers that already run one scanner per core, create the scanner with the
`parallel` option off (`parallel=False` in Python, `ScannerOptions { parallel:
false, ..ScannerOptions::default() }` in Rust, `withParallel(false)` in Java and Android, `parallel = false`
in C++, `parallel: false` in Swift, `TAPIRSCAN_PARALLEL_OFF` in C) to avoid nesting
reader threads inside worker threads. Results are identical either way. Without the option,
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
