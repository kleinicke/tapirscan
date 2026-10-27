# Browser deployment

What a site needs to serve `tapirscan/browser` and `tapirscan/camera` (preview).
For the API, see the [JavaScript README](../bindings/javascript/README.md#browser-apps-react-and-svelte).

## Bundle one mode

Bundlers emit every WASM file an entry references into the build, although a page
downloads only the selected mode. For deployment and service-worker precache
size, import a single-mode entry:

| Entry                         | WASM emitted     | Size (gzip)     |
| ----------------------------- | ---------------- | --------------- |
| `tapirscan/browser/low`       | `low.wasm`       | 2.0 MB (0.8 MB) |
| `tapirscan/browser/medium`    | `medium.wasm`    | 2.5 MB (1.0 MB) |
| `tapirscan/browser/high`      | `high.wasm`      | 2.7 MB (1.0 MB) |
| `tapirscan/browser/very-high` | `very-high.wasm` | 2.7 MB (1.0 MB) |
| `tapirscan/browser`           | all modes, Turbo | 18 MB (7 MB)    |

Vite bundles the worker and WASM itself; [Angular](#angular) production builds
need the app to start the worker.

## Load it fast

- **Compress `.wasm`.** gzip gives the sizes in the table above; brotli is about
  a quarter smaller. Browsers decompress both themselves; the app changes
  nothing. Netlify, Vercel and Cloudflare compress automatically. Plain
  nginx needs `application/wasm` in `gzip_types`, and brotli needs the
  `ngx_brotli` module.
- **Cache it for good.** Bundlers put a content hash in the file name, so serve
  those files with `Cache-Control: public, max-age=31536000, immutable`; repeat
  visits then load nothing.
- **Start early.** Constructing a `Scanner` starts the download in the
  background. Create it when the app starts, or at the latest together with the
  camera, so the download overlaps the permission prompt, and keep it for later
  scans. See [scanners in apps and on phones](../bindings/javascript/README.md#scanners-in-apps-and-on-phones).

## Serve the worker from the page's origin

Scanning runs in a module worker. Browsers start workers only from the page's
own origin, so the bundled JavaScript that contains the worker must be served
from the application's origin, not from a CDN or object-storage domain. The
WASM files may live elsewhere: point `wasmBaseUrl` at them and allow the page's
origin with CORS. The bundler still emits the entry's own copy.

Without a bundler, for example as static files of a server-rendered app,
copy the package's `dist/` directory and the WASM files you use into a `wasm/`
directory beside it, keeping that layout, and import the entry module (in
Django, an app's `static/tapirscan/` directory, served from the site's own
origin):

```html
<script type="module">
  import { Scanner } from "/static/tapirscan/dist/browser-medium.js";
</script>
```

The worker finds its WASM at `../wasm/` relative to `dist/`. Any MIME type works
for `.wasm`; `.js` files must be served as JavaScript.

## Content Security Policy

Compiling WebAssembly needs `'wasm-unsafe-eval'` in `script-src`. It does not
allow JavaScript `eval`:

```
Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval'
```

Without it, every scan rejects with a WebAssembly compile error. Also check:

- `worker-src` (or `script-src`, when `worker-src` is absent) allows `'self'`.
- `connect-src` allows the WASM origin when `wasmBaseUrl` points elsewhere.
- A `Permissions-Policy` header, if the site sends one, allows `camera=(self)`
  for `tapirscan/camera`.

Camera access also needs HTTPS; `localhost` counts as secure during
development.

## Angular

`ng serve` works without configuration. Production builds of the application
builder (Angular 22) do not emit workers from `node_modules`, so start the worker
from your own code, which Angular bundles, and copy your mode's WASM under
`build.options.assets` in `angular.json`:

```ts
// scanner.worker.ts
import "tapirscan/browser/medium/worker";
```

```ts
import { Scanner } from "tapirscan/browser/medium";

const scanner = new Scanner({
  wasmBaseUrl: new URL("tapirscan/", document.baseURI),
  worker: () => new Worker(new URL("./scanner.worker", import.meta.url), { type: "module" }),
});
```

Create it once, in a root service that every scanning dialog shares, as in
[this example](../bindings/javascript/README.md#angular-root-service), not in the
dialog component.

```json
{ "glob": "medium.wasm", "input": "node_modules/tapirscan/wasm", "output": "tapirscan" }
```

Each entry has a worker module: `tapirscan/browser/low/worker`, `/medium/worker`,
`/high/worker`, `/very-high/worker`, and `tapirscan/browser/worker` for every mode
and Turbo preset. With `wasmBaseUrl`, the worker loads the scanner's mode or
preset from that directory, so copy the WASM files you select; without it, the
worker module must bundle them. Without `worker`, a production build reports
that its worker failed to start.

## Check support first

`tapirscan/support` imports nothing, so a page can check before it downloads
anything and keep another scanner where Tapirscan cannot run:

```js
import { browserSupported } from "tapirscan/support";

if (browserSupported()) {
  const { Scanner } = await import("tapirscan/browser/medium");
  // ...
}
```

`browserSupported()` checks the features a browser scanner needs: WebAssembly
SIMD, module workers, `OffscreenCanvas` and `createImageBitmap`.
`wasmSupported()` checks only SIMD, for the core. A scanner created anyway starts no
worker and downloads nothing: its `ready` and scans reject with the `ScannerError`
code `"unsupported"`, as does the core's `Scanner.create`.

A dynamic `import()` keeps the scanner's code out of the initial bundle, and the WASM
is fetched only when a `Scanner` is constructed. Service workers that precache the
build (Angular's `ngsw-config.json`, Workbox) should cache the WASM on first use
instead, for example with an asset group that has `"installMode": "lazy"` for
`/tapirscan/*.wasm`, so installing the app does not download it.
