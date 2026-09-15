export type CameraCapabilities = MediaTrackCapabilities & {
  focusMode?: string[];
  torch?: boolean;
  zoom?: { min: number; max: number; step: number };
};
export function capabilities(track: MediaStreamTrack): CameraCapabilities {
  try {
    return track.getCapabilities?.() ?? {};
  } catch {
    return {};
  }
}
/** Capture the delivered video pixels directly, as in the unpaired camera path. */
export async function videoFrame(video: HTMLVideoElement): Promise<ImageBitmap> {
  if (video.readyState < 2 || !video.videoWidth || !video.videoHeight) {
    throw Error("The camera has not delivered a frame yet");
  }
  const canvas = document.createElement("canvas");
  canvas.width = video.videoWidth;
  canvas.height = video.videoHeight;
  const context = canvas.getContext("2d");
  if (!context) throw Error("Unable to capture the video frame");
  context.drawImage(video, 0, 0);
  return createImageBitmap(canvas);
}
export function scanDimensions(width: number, height: number, limit: number): [number, number] {
  const ratio = Math.min(
    1,
    limit / Math.max(width, height),
    Math.sqrt((32 * 1024 * 1024) / (width * height)),
  );
  return [Math.max(3, Math.floor(width * ratio)), Math.max(3, Math.floor(height * ratio))];
}
