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
   memory and copies.
5. **Hand over decoded 8-bit pixels.** Gray, RGB and RGBA are all fine: the scanner
   converts to luma itself, so do not convert color first. Skip encode/decode
   round trips, such as drawing to a canvas or saving a JPEG, when you already hold
   pixels.

## What it costs

Per-scan times on 960×960 photo crops from the project's variation set (mixed
formats), native library, one scan at a time. With several 2D formats enabled,
native builds run those readers on separate threads ("parallel", the default; see
[threads](#threads-and-servers)):

| Mode and formats    | M1 Pro, one thread | M1 Pro, parallel | Ryzen 7 5800X, one thread | Ryzen, parallel |
| ------------------- | -----------------: | ---------------: | ------------------------: | --------------: |
| Low, retail         |             4.8 ms |           4.8 ms |                    5.1 ms |          5.1 ms |
| Medium, retail      |              33 ms |            33 ms |                     36 ms |           36 ms |
| Low, all formats    |              31 ms |            18 ms |                     35 ms |           21 ms |
| Medium, all formats |              49 ms |            37 ms |                     62 ms |           50 ms |

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
`scan()` costs about 2 ms extra per call. Decode straight to gray when you can, for
example `sharp(file).greyscale().raw()` with `channels: 1`.

## Python

- **Inputs, cheapest first:** contiguous uint8 NumPy arrays (gray, RGB or RGBA);
  uint8 tensors; float arrays and tensors (validated and rounded, multithreaded for
  large arrays); Pillow images (one `tobytes()` copy). CUDA tensors are converted
  on the GPU and transferred once, so scan tensors where they already are. Moving
  a CPU tensor to the GPU only for scanning does not help uint8 tensors; for a
  large float tensor it saved about 9 ms at 12 MP on a Ryzen 7 5800X with an
  RTX 5060 Ti (the upload costs almost as much as the CPU conversion).
- **OpenCV** images are BGR: pass `color_order="BGR"` instead of calling
  `cv2.cvtColor`, or read with `cv2.IMREAD_GRAYSCALE` when color is not needed.
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
- **OpenCV `cv::Mat`:** there is no BGR layout; convert to gray, which is also the
  cheapest input:

  ```cpp
  cv::Mat gray;
  cv::cvtColor(frame, gray, cv::COLOR_BGR2GRAY);
  auto image = tapirscan::Image::gray(gray.data, gray.step * gray.rows, gray.cols, gray.rows)
                   .with_stride(gray.step);
  auto result = scanner.scan(image);
  ```

- **Rust on x86-64:** build with `RUSTFLAGS="-C target-cpu=x86-64-v2"` or in
  `.cargo/config.toml` (`rustflags = ["-C", "target-cpu=x86-64-v2"]`); scans are
  20-30% faster. The prebuilt native library already uses it. With the `image`
  crate, `GrayImage`, `RgbImage`, `RgbaImage` and 8-bit `DynamicImage`s are
  borrowed without copying.

## Threads and servers

A scanner serves one call at a time; separate scanners run concurrently, so use one
per thread. Native builds additionally run several enabled 2D readers of one scan on
separate threads, which shortens a single scan with all formats by up to 1.75×. On
servers that already run one scanner per core, set `TAPIRSCAN_THREADS=1` to keep
each scan on one thread; this saves the few percent of extra CPU time that the
parallel readers cost. WebAssembly scans always use one thread.

## Mobile apps

- **Web apps and WebViews:** use `tapirscan/browser`, as above.
- **iOS:** the C library builds as a static library for devices and the simulator;
  call it from Swift through the C header.
- **Android:** call the C library through JNI from Kotlin or Java (64-bit ABIs);
  there is no prebuilt AAR yet. 32-bit devices can use the browser build in a
  WebView.
