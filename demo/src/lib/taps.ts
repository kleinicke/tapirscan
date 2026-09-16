type Point = { clientX: number; clientY: number; timeStamp: number };
const distance = (a: Point, b: Point) => Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY);

/** Recognizes two short single-finger taps, allowing normal finger jitter. */
export class DoubleTap {
  private start: Point | undefined;
  private previous: Point | undefined;

  down(point: Point) {
    this.start = point;
  }

  move(point: Point): boolean {
    if (!this.start) return false;
    if (distance(this.start, point) <= 12) return true;
    this.cancel();
    return false;
  }

  up(point: Point): boolean {
    const start = this.start;
    this.start = undefined;
    if (!start || point.timeStamp - start.timeStamp > 300 || distance(start, point) > 12) {
      this.previous = undefined;
      return false;
    }
    const previous = this.previous;
    if (
      previous &&
      point.timeStamp - previous.timeStamp <= 400 &&
      distance(previous, point) <= 28
    ) {
      this.previous = undefined;
      return true;
    }
    this.previous = point;
    return false;
  }

  cancel() {
    this.start = undefined;
    this.previous = undefined;
  }
}
