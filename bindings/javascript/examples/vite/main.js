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
input.onchange = async () => {
  const file = input.files[0];
  if (!file) return;
  status.textContent = "Scanning…";
  try {
    const { barcodes } = await scanner.scan(file);
    status.textContent = barcodes.length
      ? barcodes.map((barcode) => `${barcode.format} ${barcode.text}`).join("\n")
      : "No barcode found.";
  } catch (error) {
    status.textContent = error.message;
  }
};
