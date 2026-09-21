// Quagga2's browser UMD wrapper references `window` even on its raw worker path.
// Supply its global alias without providing DOM APIs or moving work to the UI.
Object.defineProperty(globalThis, "window", { value: globalThis, configurable: true });
