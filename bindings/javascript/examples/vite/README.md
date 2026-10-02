# Vite worker example

Copy this directory into your application workspace, then run:

```sh
npm install
npm run dev
```

Choose a barcode photo. The page transfers decoded pixels to one reusable Medium
scanner in a worker; the worker and its WASM memory live until the page closes.
The WASM URL import lets Vite manage the asset without a copy step or plugin.
Retail formats are enabled by default.

For a local unpublished Tapirscan version, install its npm tarball instead of
the registry dependency before running the example:

```sh
npm install /absolute/path/to/tapirscan-1.3.0.tgz
```

Test production hosting, including a subpath:

```sh
npm run build -- --base=/scanner/
npm run preview
```

Open `/scanner/` on the preview server. In SvelteKit, create this worker from
`onMount` and return a cleanup function that calls `worker.terminate()`. Terminating
the worker releases its scanner and WASM memory. Import the WASM inside the worker
as shown, so no browser initialization runs during server rendering.
