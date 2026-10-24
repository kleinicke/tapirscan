# Vite example

Copy this directory into your application workspace, then run:

```sh
npm install
npm run dev
```

Choose a barcode photo. `tapirscan/browser/medium` decodes the file and scans it
in a worker; Vite bundles the worker and `medium.wasm` without plugins or copy
steps. Importing `tapirscan/browser` instead bundles all eight WASM files.
The only configuration is `optimizeDeps.exclude` in `vite.config.js`, which the
development server needs to serve those files from the package. Retail formats
are enabled by default.

To use a package built from source, install its npm tarball before running the
example:

```sh
npm install /absolute/path/to/tapirscan-X.Y.Z.tgz
```

Test production hosting, including a subpath:

```sh
npm run build -- --base=/scanner/
npm run preview -- --base=/scanner/
```

Open `/scanner/` on the preview server. For Svelte and SvelteKit, see the
[package README](../../README.md#browser-apps-react-and-svelte).
