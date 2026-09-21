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
  // Linear readers may report a scan line, not a region with positive area.
  // Give only degenerate axes a small tolerance for location matching.
  a = {
    ...a,
    x: a.x - (a.width < 1 ? 2 : 0),
    y: a.y - (a.height < 1 ? 2 : 0),
    width: Math.max(4, a.width),
    height: Math.max(4, a.height),
  };
  b = {
    ...b,
    x: b.x - (b.width < 1 ? 2 : 0),
    y: b.y - (b.height < 1 ? 2 : 0),
    width: Math.max(4, b.width),
    height: Math.max(4, b.height),
  };
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
  private lastSeen = new Map<number, number>();
  update(
    regions: LabelRegion[],
    scanners: LabelScanner[],
    width: number,
    height: number,
    unit: number,
    context: string,
    now = performance.now(),
  ) {
    if (context !== this.context) {
      this.previous = [];
      this.lastSeen.clear();
      this.context = context;
    }
    const groups: { text: string; anchor: Box; members: LabelRegion[] }[] = [];
    for (const region of regions) {
      const box = bounds(region.polygon);
      const group = groups.find(
        (g) =>
          g.text === region.text &&
          (!g.members.some((m) => m.scanner === region.scanner) || region.polygon.length <= 2) &&
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
      // Place outside the entire group's reported bounds, rather than letting a
      // later reader's larger outline invalidate slots around the first reader.
      const anchor = bounds(group.members.flatMap((member) => member.polygon)),
        cx = anchor.x + anchor.width / 2,
        cy = anchor.y + anchor.height / 2;
      const alternatives: Box[] = [];
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
      // Score only fresh positions tied to the current barcode, never reuse an
      // accumulated offset. A bounded movement penalty gently favors nearby slots.
      const reference = old
        ? {
            x:
              old.x + group.anchor.x + group.anchor.width / 2 - old.anchor.x - old.anchor.width / 2,
            y:
              old.y +
              group.anchor.y +
              group.anchor.height / 2 -
              old.anchor.y -
              old.anchor.height / 2,
          }
        : undefined;
      const scored = alternatives.flatMap((box, index) => {
        const side = index % 4;
        return [0, -8, 8, -16, 16, -24, 24].map((shift) => {
          const candidate = {
            ...box,
            x: box.x + (side < 2 ? shift * unit : 0),
            y: box.y + (side >= 2 ? shift * unit : 0),
          };
          const movement = reference
            ? Math.min(80, Math.hypot(candidate.x - reference.x, candidate.y - reference.y) / unit)
            : 0;
          return {
            box: candidate,
            score: Math.floor(index / 4) * 41 + side * 2 + Math.abs(shift) * 0.25 + movement * 0.4,
          };
        });
      });
      const position = scored
        .filter((candidate) => valid(candidate.box))
        .sort((a, b) => a.score - b.score)
        .at(0)?.box;
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
      }
    return { placed, hidden: groups.length - placed.length };
  }
}
