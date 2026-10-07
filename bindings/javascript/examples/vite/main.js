import { Scanner } from "tapirscan/browser";

const input = document.querySelector("input");
const status = document.querySelector('[role="status"]');
// Scanning runs in a worker, so the page stays responsive.
const scanner = new Scanner();

scanner.ready.then(
  () => {
    input.disabled = false;
    status.textContent = "Choose a photo.";
  },
  (error) => {
    status.textContent = error.message;
  },
);
// Only the latest selection may update the page; scans can finish out of order.
let latest = 0;
input.onchange = async () => {
  const file = input.files[0];
  if (!file) return;
  const request = ++latest;
  status.textContent = "Scanning…";
  try {
    const { barcodes } = await scanner.scan(file);
    if (request !== latest) return;
    status.textContent = barcodes.length
      ? barcodes.map((barcode) => `${barcode.format} ${barcode.text}`).join("\n")
      : "No barcode found.";
  } catch (error) {
    if (request === latest) status.textContent = error.message;
  }
};
