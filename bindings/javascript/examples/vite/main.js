const input = document.querySelector("input");
const status = document.querySelector('[role="status"]');
const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
worker.onmessage = ({ data }) => {
  input.disabled = Boolean(data.fatal);
  status.textContent =
    data.error ?? (data.values ? JSON.stringify(data.values) : "Choose a photo.");
};
worker.onerror = (event) => {
  status.textContent = event.message;
  input.disabled = true;
};
input.onchange = async () => {
  const file = input.files[0];
  if (!file) return;
  input.disabled = true;
  status.textContent = "Scanning…";
  try {
    const bitmap = await createImageBitmap(file);
    let image;
    try {
      const canvas = document.createElement("canvas");
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const context = canvas.getContext("2d");
      context.drawImage(bitmap, 0, 0);
      image = context.getImageData(0, 0, canvas.width, canvas.height);
    } finally {
      bitmap.close();
    }
    worker.postMessage(image, [image.data.buffer]);
  } catch (error) {
    input.disabled = false;
    status.textContent = error.message;
  }
};
