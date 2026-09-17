/** Group physical barcode instances and keep their labels stable between partial results. */
export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface LabelRegion {
  polygon: readonly (readonly number[])[];
  text: string;
  scanner: string;
  scanMs: number;
}
export interface LabelScanner {
  id: string;
  label: string;
  color: string;
  pending: boolean;
}
export interface GroupLabel extends Box {
  id: number;
  text: string;
  displayText: string;
  anchor: Box;
  rows: (LabelScanner & { found: boolean; offset: number })[];
}
export function overlaps(a: Box, b: Box): boolean {
  return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}
function bounds(polygon: readonly (readonly number[])[]): Box {
  const xs = polygon.map((p) => p[0]),
    ys = polygon.map((p) => p[1]);
  const x = Math.min(...xs),
    y = Math.min(...ys);
  return { x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y };
}
function sameLocation(a: Box, b: Box) {
  const intersection =
    Math.max(0, Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x)) *
    Math.max(0, Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y));
  return (
    intersection > 0 &&
    intersection / Math.max(1, Math.min(a.width * a.height, b.width * b.height)) >= 0.4
  );
}
function padded(box: Box, gap: number) {
  return {
    x: box.x - gap,
    y: box.y - gap,
    width: box.width + gap * 2,
    height: box.height + gap * 2,
  };
}
export class LabelLayout {
  private previous: GroupLabel[] = [];
  private context = "";
  private nextId = 0;
  private relocations = new Map<number, { box: Box; since: number }>();
  private lastSeen = new Map<number, number>();
  update(
    regions: LabelRegion[],
    scanners: LabelScanner[],
    width: number,
    height: number,
    unit: number,
    context: string,
    now = performance.now(),
    holdRelocations = false,
  ) {
    if (context !== this.context) {
      this.previous = [];
      this.relocations.clear();
      this.lastSeen.clear();
      this.context = context;
    }
    const groups: { text: string; anchor: Box; members: LabelRegion[] }[] = [];
    for (const region of regions) {
      const box = bounds(region.polygon);
      const group = groups.find(
        (g) =>
          g.text === region.text &&
          !g.members.some((m) => m.scanner === region.scanner) &&
          sameLocation(g.anchor, box),
      );
      if (group) group.members.push(region);
      else groups.push({ text: region.text, anchor: box, members: [region] });
    }
    this.previous = this.previous.filter(
      (label) => now - (this.lastSeen.get(label.id) ?? 0) < 1200,
    );
    const matched = new Set<number>();
    const candidates = groups
      .map((group) => {
        const old = this.previous.find(
          (p) =>
            !matched.has(p.id) && p.text === group.text && sameLocation(p.anchor, group.anchor),
        );
        if (old) matched.add(old.id);
        return { group, old };
      })
      .sort((a, b) => (a.old?.id ?? Infinity) - (b.old?.id ?? Infinity));
    const occupied: Box[] = regions.map((r) => padded(bounds(r.polygon), 4 * unit));
    occupied.push({ x: 0, y: 0, width, height: 58 * unit });
    const placed: GroupLabel[] = [];
    for (const { group, old } of candidates) {
      const displayText = group.text.length > 24 ? group.text.slice(0, 21) + "…" : group.text;
      // Fixed name slots prevent later results shifting the attribution line.
      const nameWidth = scanners.reduce((sum, scanner) => sum + scanner.label.length * 5.5 + 8, 0);
      const w = Math.max(displayText.length * 7.3 + 12, nameWidth + 4) * unit;
      const h = 35 * unit,
        gap = 6 * unit;
      const valid = (b: Box) =>
        b.x >= gap &&
        b.y >= gap &&
        b.x + b.width <= width - gap &&
        b.y + b.height <= height - gap &&
        !occupied.some((o) => overlaps(b, o));
      let position: Box | undefined;
      if (old && old.width === w && old.height === h && valid(old)) position = old;
      const anchor = group.anchor,
        cx = anchor.x + anchor.width / 2,
        cy = anchor.y + anchor.height / 2;
      const alternatives: Box[] = [];
      // Try small corrections around the current position before considering another side.
      if (old)
        for (const radius of [4, 8, 16, 24]) {
          for (const [dx, dy] of [
            [0, -1],
            [0, 1],
            [-1, 0],
            [1, 0],
            [-1, -1],
            [1, 1],
          ])
            alternatives.push({
              x: old.x + dx * radius * unit,
              y: old.y + dy * radius * unit,
              width: w,
              height: h,
            });
        }
      for (let ring = 0; ring < 6; ring++) {
        const offset = ring * (h + gap);
        for (const [x, y] of [
          [cx - w / 2, anchor.y - h - gap - offset],
          [cx - w / 2, anchor.y + anchor.height + gap + offset],
          [anchor.x - w - gap, cy - h / 2 - offset],
          [anchor.x + anchor.width + gap, cy - h / 2 + offset],
        ])
          alternatives.push({
            x: Math.max(gap, Math.min(width - w - gap, x)),
            y,
            width: w,
            height: h,
          });
      }
      if (!position) {
        const available = alternatives.filter(valid);
        if (old)
          available.sort(
            (a, b) => Math.hypot(a.x - old.x, a.y - old.y) - Math.hypot(b.x - old.x, b.y - old.y),
          );
        position = available.at(0);
        if (old && position && Math.hypot(position.x - old.x, position.y - old.y) > 24 * unit) {
          const pending = this.relocations.get(old.id);
          if (
            !pending ||
            Math.hypot(pending.box.x - position.x, pending.box.y - position.y) > 24 * unit
          ) {
            this.relocations.set(old.id, { box: position, since: now });
            position = undefined;
          } else if (holdRelocations || now - pending.since < 650) position = undefined;
        }
      } else if (old) this.relocations.delete(old.id);
      if (old) {
        this.lastSeen.set(old.id, now);
        // Track the detection even when its label is temporarily hidden by a collision.
        old.anchor = group.anchor;
      }
      if (!position) continue;
      let nameOffset = 6;
      const label: GroupLabel = {
        ...position,
        id: old?.id ?? this.nextId++,
        text: group.text,
        displayText,
        anchor: group.anchor,
        rows: scanners.map((scanner) => {
          const member = group.members.find((m) => m.scanner === scanner.id);
          const offset = nameOffset;
          nameOffset += scanner.label.length * 5.5 + 8;
          return { ...scanner, found: !!member, offset };
        }),
      };
      this.lastSeen.set(label.id, now);
      occupied.push(padded(label, 2 * unit));
      placed.push(label);
    }
    this.previous = [
      ...placed,
      ...this.previous.filter((old) => !placed.some((label) => label.id === old.id)),
    ];
    const retained = new Set(this.previous.map((label) => label.id));
    for (const id of this.lastSeen.keys())
      if (!retained.has(id)) {
        this.lastSeen.delete(id);
        this.relocations.delete(id);
      }
    return { placed, hidden: groups.length - placed.length };
  }
}
