# Browser deployment

What a site needs to serve `tapirscan/browser` and `tapirscan/camera`. For the API,
see the [JavaScript README](../bindings/javascript/README.md#browser-apps-react-and-svelte).

## Bundle one mode

Bundlers emit every WASM file an entry references into the build, although a page
downloads only the selected mode. For deployment and service-worker precache
size, import a single-mode entry:

| Entry                         | WASM emitted     | Size (gzip)     |
| ----------------------------- | ---------------- | --------------- |
| `tapirscan/browser/low`       | `low.wasm`       | 2.0 MB (0.8 MB) |
| `tapirscan/browser/medium`    | `medium.wasm`    | 2.6 MB (1.0 MB) |
| `tapirscan/browser/high`      | `high.wasm`      | 2.7 MB (1.0 MB) |
| `tapirscan/browser/very-high` | `very-high.wasm` | 2.7 MB (1.0 MB) |
| `tapirscan/browser`           | all modes, Turbo | 18 MB (7 MB)    |

Tested with Vite; [Angular](#angular) needs two asset entries.

## Serve the worker from the page's origin

Scanning runs in a module worker. Browsers start workers only from the page's
own origin, so the bundled JavaScript that contains the worker must be served
from the application's origin, not from a CDN or object-storage domain. The
WASM files may live elsewhere: point `wasmBaseUrl` at them and allow the page's
origin with CORS. The bundler still emits the entry's own copy.

Without a bundler, for example as static files of a server-rendered app,
copy the package's `dist/` directory and the WASM files you use into a `wasm/`
directory beside it, keeping that layout, and import the entry module:

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
builder (Angular 22) do not emit the worker from `node_modules`, so the scanner
reports that its worker failed to start. Copy the package's JavaScript and your
mode's WASM in `angular.json` under `build.options.assets`:

```json
{ "glob": "**/*.js", "input": "node_modules/tapirscan/dist", "output": "/" },
{ "glob": "medium.wasm", "input": "node_modules/tapirscan/wasm", "output": "/wasm" }
```

The worker imports other modules from `dist/`, so copy all of its `.js` files.
