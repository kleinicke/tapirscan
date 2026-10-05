import { readFile, writeFile, mkdir, copyFile } from "node:fs/promises";
const root = new URL("../../", import.meta.url);
const publicRoot = new URL("demo/public/", root);
const domain = "https://tapirscan.f-kleinicke.de";
const pages = JSON.parse(await readFile(new URL("demo/content/docs.json", root), "utf8"));
const formats = JSON.parse(await readFile(new URL("config/formats.json", root), "utf8"));
const escape = (s) =>
  String(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
const absolute = (url) => new URL(url, domain).href;
const nav = `<nav aria-label="Main"><a href="/">Demo</a><a href="/docs/">Documentation</a><a href="/formats/">Formats</a><a href="/releases/">Releases</a><a href="/license/">License</a></nav>`;
const style = `:root{color-scheme:light;font:17px/1.65 system-ui,sans-serif;background:#f4f8f5;color:#183c33}body{max-width:980px;margin:auto;padding:24px}nav{display:flex;flex-wrap:wrap;gap:12px 24px;border-bottom:1px solid #b6cec2;padding-bottom:18px}a{color:#076450;text-underline-offset:3px}h1{font-size:clamp(2rem,5vw,3rem);line-height:1.15;letter-spacing:-.035em}h2{line-height:1.3;margin-top:2rem}p{max-width:78ch}pre{padding:20px;background:#17392f;color:#effff6;overflow:auto;border-radius:10px;font-size:.85rem}code{font-family:ui-monospace,monospace}table{width:100%;border-collapse:collapse}th,td{text-align:left;padding:10px;border-bottom:1px solid #b6cec2;vertical-align:top;overflow-wrap:anywhere}footer{margin-top:48px;border-top:1px solid #b6cec2;padding-top:18px;font-size:.9rem}.table-wrap{overflow:auto}main{padding-top:20px}`;
for (const page of pages) {
  const url = `${domain}/${page.path}/`;
  const markdownUrl = `${url}index.md`;
  const md = [`# ${page.title}`, "", page.description, "", `Canonical: ${url}`, ""];
  const html = [];
  for (const section of page.sections) {
    html.push(`<section><h2>${escape(section.title)}</h2>`);
    md.push(`## ${section.title}`, "");
    if (section.text) {
      html.push(`<p>${escape(section.text)}</p>`);
      md.push(section.text, "");
    }
    if (section.code) {
      html.push(`<pre><code>${escape(section.code)}</code></pre>`);
      md.push("```" + section.lang, section.code, "```", "");
    }
    if (section.links) {
      html.push(
        "<ul>" +
          section.links
            .map(([title, link]) => `<li><a href="${escape(link)}">${escape(title)}</a></li>`)
            .join("") +
          "</ul>",
      );
      md.push(...section.links.map(([title, link]) => `- [${title}](${absolute(link)})`), "");
    }
    if (page.path === "formats" && section.title === "Format presets") {
      const rows = Object.entries({ ...formats.presets, all: formats.formats.map((x) => x[0]) });
      html.push(
        `<div class="table-wrap"><table><thead><tr><th>Preset</th><th>Formats</th></tr></thead><tbody>${rows.map(([k, v]) => `<tr><td><code>${escape(k)}</code></td><td>${escape(v.join(", "))}</td></tr>`).join("")}</tbody></table></div>`,
      );
      md.push(
        "| Preset | Formats |",
        "|---|---|",
        ...rows.map(([k, v]) => `| ${k} | ${v.join(", ")} |`),
        "",
      );
    }
    html.push("</section>");
  }
  await mkdir(new URL(`${page.path}/`, publicRoot), { recursive: true });
  await writeFile(new URL(`${page.path}/index.md`, publicRoot), md.join("\n"));
  await writeFile(
    new URL(`${page.path}/index.html`, publicRoot),
    `<!doctype html><html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${escape(page.title)} · Tapirscan</title><meta name="description" content="${escape(page.description)}"><link rel="canonical" href="${url}"><link rel="alternate" type="text/markdown" href="${markdownUrl}"><link rel="describedby" href="${domain}/llms.txt"><style>${style}</style></head><body>${nav}<main><h1>${escape(page.title)}</h1><p>${escape(page.description)}</p><p><a href="${markdownUrl}">Read as Markdown</a></p>${html.join("\n")}</main><footer>Tapirscan · <a href="/license/">MIT OR Apache-2.0</a> · <a href="https://github.com/kleinicke/tapirscan">GitHub</a></footer></body></html>\n`,
  );
}
for (const name of ["LICENSE", "LICENSE-APACHE", "LICENSE-MIT"])
  await copyFile(new URL(name, root), new URL(`license/${name}.txt`, publicRoot));
for (const language of ["javascript", "python"]) {
  let md = await readFile(new URL(`bindings/${language}/README.md`, root), "utf8");
  const base = `https://github.com/kleinicke/tapirscan/blob/main/bindings/${language}/`;
  md = md.replace(/\]\(([^)]+)\)/g, (match, link) =>
    /^(https?:|#|mailto:)/.test(link) ? match : `](${new URL(link, base).href})`,
  );
  await writeFile(
    new URL(`docs/${language}/api.md`, publicRoot),
    `> Export of the repository API guide. Check released package documentation for your installed version.\n\n${md}`,
  );
}
await writeFile(
  new URL("llms.txt", publicRoot),
  [
    "# Tapirscan",
    "",
    "> Barcode scanning for browser, Node.js and native applications. Multiple symbols, original-image geometry, four effort modes; MIT OR Apache-2.0.",
    "",
    "Tapirscan supports linear and 2D barcode formats, with Retail enabled by default. See the format guide for variants and limitations. Turbo presets remain experimental. The demo next builds may be newer than the published packages. Consult the release page before selecting a version.",
    "",
    "## Documentation",
    "",
    ...pages.map((p) => `- [${p.title}](${domain}/${p.path}/index.md): ${p.description}`),
    "",
    "## API reference",
    "",
    `- [JavaScript/TypeScript API](${domain}/docs/javascript/api.md)`,
    `- [Python API](${domain}/docs/python/api.md)`,
    "- [Source repository](https://github.com/kleinicke/tapirscan)",
    "- [npm package](https://www.npmjs.com/package/tapirscan)",
    "- [PyPI package](https://pypi.org/project/tapirscan/)",
    "",
  ].join("\n"),
);
await writeFile(
  new URL("sitemap.xml", publicRoot),
  `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${["", ...pages.map((p) => p.path + "/")].map((p) => `  <url><loc>${domain}/${p}</loc></url>`).join("\n")}\n</urlset>\n`,
);
console.log(
  `Generated ${pages.length} documentation pages, Markdown exports, licenses and sitemap.`,
);
