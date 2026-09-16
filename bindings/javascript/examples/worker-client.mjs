/** Initialize once. scan() transfers ownership of the supplied pixel buffer.
 * Await each scan before capturing the next frame; dispose() stops the worker.
 * Structured cloning yields independent mutable results on the receiving side.
 */
export async function createScannerWorker(options = {}) {
  const worker = new Worker(new URL("./scan-worker.mjs", import.meta.url), { type: "module" });
  let pending;
  let nextId = 0;
  let closed = false;
  function dispose(reason = Error("Scanner worker disposed")) {
    if (closed) return;
    closed = true;
    worker.terminate();
    pending?.reject(reason);
    pending = undefined;
  }
  worker.onerror = (event) => dispose(Error(event.message || "Scanner worker failed"));
  worker.onmessageerror = () => dispose(Error("Could not deserialize scanner result"));
  worker.onmessage = ({ data }) => {
    if (!pending || pending.id !== data.id) return;
    const { resolve, reject } = pending;
    pending = undefined;
    if (data.error) reject(Object.assign(Error(data.error.message), { code: data.error.code }));
    else resolve(data.result);
  };
  function request(type, message, transfer = []) {
    if (closed) return Promise.reject(Error("Scanner worker disposed"));
    if (pending)
      return Promise.reject(Error("Await the previous scan before sending another frame"));
    return new Promise((resolve, reject) => {
      const id = nextId++;
      pending = { id, resolve, reject };
      try {
        worker.postMessage({ id, type, ...message }, transfer);
      } catch (error) {
        pending = undefined;
        reject(error);
      }
    });
  }
  try {
    await request("init", { options });
  } catch (error) {
    dispose(error);
    throw error;
  }
  return {
    scan(image, options = {}) {
      // Send explicit storage settings; ImageData's prototype is not required in the worker.
      const pixels = {
        data: image.data,
        width: image.width,
        height: image.height,
        channels: image.channels ?? 4,
        stride: image.stride,
      };
      return request("scan", { image: pixels, options }, [image.data.buffer]);
    },
    dispose() {
      dispose();
    },
  };
}
