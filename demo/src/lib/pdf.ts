import { getDocument, GlobalWorkerOptions, type PDFDocumentProxy } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

GlobalWorkerOptions.workerSrc = workerUrl;

/** PDF pages remain whole and use their stored orientation. */
export async function openPdf(file: File): Promise<PDFDocumentProxy> {
  const base = `${import.meta.env.BASE_URL}pdf/`;
  const task = getDocument({
    data: new Uint8Array(await file.arrayBuffer()),
    cMapUrl: `${base}cmaps/`,
    cMapPacked: true,
    standardFontDataUrl: `${base}standard_fonts/`,
    wasmUrl: `${base}wasm/`,
  });
  try {
    return await task.promise;
  } catch (reason) {
    await task.destroy();
    throw reason;
  }
}

export async function renderPdfPage(document: PDFDocumentProxy, number: number): Promise<Blob> {
  const page = await document.getPage(number);
  const original = page.getViewport({ scale: 1 });
  // 300 dpi, bounded for mobile memory. Scanner resolution limits still apply.
  const scale = Math.min(
    300 / 72,
    4096 / Math.max(original.width, original.height),
    Math.sqrt(16_000_000 / (original.width * original.height)),
  );
  const viewport = page.getViewport({ scale });
  const canvas = window.document.createElement("canvas");
  canvas.width = Math.ceil(viewport.width);
  canvas.height = Math.ceil(viewport.height);
  try {
    await page.render({ canvas, viewport, background: "rgb(255,255,255)" }).promise;
    return await new Promise<Blob>((resolve, reject) => {
      canvas.toBlob((blob) => {
        if (blob) resolve(blob);
        else reject(new Error("PDF page could not be rendered"));
      }, "image/png");
    });
  } finally {
    canvas.width = canvas.height = 0;
    page.cleanup();
  }
}
