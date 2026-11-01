// Serve the package directory and open /examples/scan-session.html on localhost.
import { Scanner } from "../dist/browser-medium.js";
import { CameraScanner } from "../dist/camera.js";
import { ScanSession } from "../dist/session.js";

const mode = document.querySelector("#mode");
const start = document.querySelector("#start");
const stop = document.querySelector("#stop");
const photo = document.querySelector("#photo");
const status = document.querySelector("#status");
const candidates = document.querySelector("#candidates");
const entries = document.querySelector("#entries");
const undo = document.querySelector("#undo");
const finish = document.querySelector("#finish");
const result = document.querySelector("#result");
const instructions = document.querySelector("#instructions");
const scanner = new Scanner();
let session = new ScanSession();
let visible = [];
let candidateSignature = "";
let generation = 0;
let loadingPhoto = false;
const camera = new CameraScanner(document.querySelector("video"), scanner, {
  confirmations: 2,
  onScan: (scan) => showCandidates(scan.barcodes),
  onDetect: (barcode) => {
    if (session.mode === "list" || visible.length === 1) accept(barcode);
  },
  onError: (error) => {
    status.textContent = `Camera stopped: ${error.message}. You can still choose a photo.`;
    render();
  },
});

function showCandidates(barcodes) {
  const identities = new Map();
  for (const { format, text } of barcodes)
    identities.set(JSON.stringify([format, text]), { format, text });
  visible = [...identities.values()];
  const signature = JSON.stringify([visible, session.finished]);
  if (signature === candidateSignature) return;
  candidateSignature = signature;
  candidates.replaceChildren(
    ...visible.map((barcode) => {
      const item = document.createElement("li");
      const choose = document.createElement("button");
      choose.textContent = `Accept ${barcode.format}: ${barcode.text || "(empty text)"}`;
      choose.disabled = session.finished;
      choose.onclick = () => accept(barcode);
      item.append(choose);
      return item;
    }),
  );
}

function accept(barcode, repeat = false) {
  if (!session.accept(barcode, { repeat })) {
    if (!session.finished)
      status.textContent = "Already on the list. Use Add another for an extra unit.";
    return;
  }
  status.textContent = `Accepted ${barcode.format}: ${barcode.text || "(empty text)"}.`;
  if (session.finished) complete();
  else render();
}

function complete() {
  generation++; // Ignore any pending file result or camera-start completion.
  loadingPhoto = false;
  camera.stop();
  result.textContent = JSON.stringify(session.finish(), null, 2);
  result.hidden = false;
  status.textContent = `Finished with ${session.entries.length} distinct item(s).`;
  render();
}

function render() {
  document.querySelector("video").hidden = !camera.running;
  start.disabled = session.finished || camera.running || loadingPhoto;
  stop.disabled = !camera.running;
  photo.disabled = session.finished || loadingPhoto;
  undo.disabled = !session.canUndo;
  finish.disabled = session.finished || loadingPhoto;
  mode.disabled = camera.running || loadingPhoto || session.entries.length > 0;
  entries.replaceChildren(
    ...session.entries.map((entry) => {
      const item = document.createElement("li");
      const label = document.createElement("span");
      label.textContent = `${entry.format}: ${entry.text || "(empty text)"} — ${entry.quantity} unit(s) `;
      const add = document.createElement("button");
      add.textContent = "Add another";
      add.setAttribute("aria-label", `Add another ${entry.text || entry.format}`);
      add.disabled = session.finished;
      add.onclick = () => accept(entry, true);
      item.append(label, add);
      return item;
    }),
  );
  showCandidates(visible);
}

start.onclick = async () => {
  const current = ++generation;
  status.textContent = "Opening camera…";
  const opening = camera.start();
  render();
  try {
    const opened = await opening;
    if (current !== generation) return;
    status.textContent = opened ? "Aim at a barcode." : "Camera is paused or stopped.";
  } catch (error) {
    if (current !== generation) return;
    status.textContent = `Camera unavailable: ${error.message}. Choose a photo instead.`;
  }
  render();
};
stop.onclick = () => {
  generation++;
  camera.stop();
  showCandidates([]);
  status.textContent = "Camera stopped. Your accepted items are kept.";
  render();
};
photo.onchange = async () => {
  const file = photo.files[0];
  if (!file || session.finished) return;
  const current = ++generation;
  camera.stop();
  loadingPhoto = true;
  showCandidates([]);
  status.textContent = "Reading photo…";
  render();
  try {
    const scan = await scanner.scan(file);
    if (current !== generation) return;
    showCandidates(scan.barcodes);
    status.textContent = visible.length
      ? "Choose a code below."
      : "No barcode read. Try a clearer photo or the camera.";
    if (session.mode === "list") {
      for (const barcode of visible) accept(barcode);
    } else if (visible.length === 1) accept(visible[0]);
  } catch (error) {
    if (current === generation) status.textContent = `Photo could not be read: ${error.message}`;
  } finally {
    photo.value = "";
    if (current === generation) {
      loadingPhoto = false;
      render();
    }
  }
};
undo.onclick = () => {
  if (session.undo()) status.textContent = "Last addition undone.";
  render();
};
finish.onclick = complete;
function reset() {
  generation++;
  camera.stop();
  loadingPhoto = false;
  session = new ScanSession({ mode: mode.value });
  result.hidden = true;
  result.textContent = "";
  showCandidates([]);
  instructions.textContent =
    session.mode === "single"
      ? "One visible code is accepted automatically. If several are visible, choose one below."
      : "New codes are added once. Add another unit explicitly, undo mistakes, then finish.";
  status.textContent = "Ready to scan.";
  render();
}
mode.onchange = reset;
document.querySelector("#reset").onclick = reset;
// The page owns the scanner for its lifetime; hiding a page pauses the camera.
render();
