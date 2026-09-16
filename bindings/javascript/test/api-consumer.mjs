// Shared real-fixture consumer for clean Node and browser package installations.
function check(condition, message) {
  if (!condition) throw Error(message);
}
function sortedKeys(_key, value) {
  return value && typeof value === "object" && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b)))
    : value;
}
function equal(actual, expected, message) {
  check(JSON.stringify(actual, sortedKeys) === JSON.stringify(expected, sortedKeys), message);
}
function checkGeometry(result, fixture) {
  if (!fixture.geometry) return;
  const reads = [...result.barcodes].sort((a, b) => a.rect.left - b.rect.left);
  equal(reads.length, fixture.geometry.length, `${fixture.name}: geometry count`);
  reads.forEach((read, i) => {
    const expected = fixture.geometry[i];
    equal(read.eanAddOn ?? null, expected.eanAddOn, `${fixture.name}: physical association`);
    check(
      Math.abs(Math.min(...read.polygon.map(([x]) => x)) - expected.left) <= 4,
      `${fixture.name}: main left edge`,
    );
    check(
      Math.abs(Math.max(...read.polygon.map(([x]) => x)) - expected.right) <= 4,
      `${fixture.name}: main right edge excludes supplement`,
    );
  });
}
export async function runApiChecks(Scanner, fixtures, options = {}) {
  const observations = [];
  for (const mode of ["low", "medium", "high", "very-high"]) {
    for (const fixture of fixtures) {
      const scanner = await Scanner.create({
        ...options,
        mode,
        formats: fixture.formats,
        eanAddOnPolicy: fixture.eanAddOnPolicy ?? "Ignore",
      });
      let result;
      try {
        for (const debug of [false, true]) {
          result = scanner.scan(
            {
              data: new Uint8Array(fixture.data),
              width: fixture.width,
              height: fixture.height,
              channels: 1,
            },
            { debug },
          );
          checkGeometry(result, fixture);
          equal(
            [...result.values].sort(),
            fixture.expected.map((b) => b.text).sort(),
            `${mode}/${fixture.name}: payloads`,
          );
          if ("unfinished" in fixture)
            equal(result.unfinished, fixture.unfinished, `${fixture.name}: work limit`);
          const actual = result.barcodes.map((b) => {
            check(
              (Array.isArray(fixture.formats) ? fixture.formats : [fixture.formats]).includes(
                b.format,
              ) && b.support > 0,
              "format and support",
            );
            check(b.polygon.length === 4 && b.rect.width > 0 && b.rect.height > 0, "geometry");
            check(
              b.polygon.every(
                ([x, y]) => x >= 0 && y >= 0 && x <= fixture.width && y <= fixture.height,
              ),
              "source coordinates",
            );
            check(Object.isFrozen(b) && Object.isFrozen(b.polygon), "immutable result");
            if (b.payloadBytes) check(Object.isFrozen(b.payloadBytes), "immutable payload bytes");
            if (b.structuredAppend)
              check(Object.isFrozen(b.structuredAppend), "immutable metadata");
            return {
              text: b.text,
              ...(fixture.expected.some((e) => "gs1" in e) ? { gs1: b.gs1 } : {}),
              ...(fixture.expected.some((e) => "eanAddOn" in e)
                ? { eanAddOn: b.eanAddOn ?? null }
                : {}),
              ...(fixture.expected.some((e) => "format" in e) ? { format: b.format } : {}),
              ...(fixture.expected.some((e) => e.text === b.text && "payloadBytes" in e)
                ? { payloadBytes: b.payloadBytes }
                : {}),
              ...(b.structuredAppend ? { structuredAppend: b.structuredAppend } : {}),
            };
          });
          const order = (a, b) =>
            a.text.localeCompare(b.text) || (a.eanAddOn ?? "").localeCompare(b.eanAddOn ?? "");
          equal(actual.sort(order), [...fixture.expected].sort(order), `${fixture.name}: metadata`);
          check(Boolean(result.debug) === debug, "debug selection");
          if (debug && fixture.expectUnread)
            check(result.debug.regions.undecoded.length > 0, `${fixture.name}: unread evidence`);
          if (!debug)
            observations.push({
              mode,
              name: fixture.name,
              unfinished: result.unfinished,
              barcodes: result.barcodes.map(({ text, format, support, polygon, eanAddOn }) => ({
                text,
                format,
                support,
                eanAddOn: eanAddOn ?? null,
                polygon,
              })),
            });
        }
      } finally {
        scanner.dispose();
      }
      equal(
        [...result.values].sort(),
        fixture.expected.map((b) => b.text).sort(),
        "result lifetime",
      );
    }
  }
  return observations;
}
