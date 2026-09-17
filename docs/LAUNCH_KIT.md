# Tapirscan launch kit

Prepared 16 September 2026. Drafts only; nothing has been posted or sent.

## Positioning

**Open-source barcode scanning for difficult angles, running locally in your browser or Python application.**

Start with developers building product lookup, inventory, document-processing and camera applications. Lead with an observable problem and the interactive demo. Use the AI development story for a second conversation about engineering and evaluation.

The initial objective is five independent developers trying their own images and two concrete integration conversations. These are campaign targets, not forecasts. Record reproducible failures and successful integrations alongside traffic and stars.

The supported launch claims are orientation-aware scanning, multiple symbols, source-image geometry, four effort modes, local processing, an MIT license, and JavaScript/Python packages. Be explicit that EAN-13, UPC-A, EAN-8 and UPC-E are supported; formats outside this retail group remain experimental.

“Much more reliable than ZXing/ZBar” needs a public paired comparison of the released versions. The development observations in [the blog](../BLOG_POST.md) and integration checks are not that comparison. We can launch now using the demo and concrete examples, then publish stronger quantified results when the evidence exists. Follow [the benchmark protocol](BENCHMARKS.md), including wrong reads and runtime as well as successful reads.

## LinkedIn: initial post

I could clearly see the barcode. The scanner returned nothing.

That frustration led me to build Tapirscan: an open-source barcode scanning library focused on EAN-13, the familiar 13-digit barcodes on retail products.

It finds barcode regions, estimates their angle, and samples across the bars. It can return multiple barcodes and their positions in the original image. It runs locally in the browser through WebAssembly, or in Python, with a Rust core.

There’s a live demo where you can compare Tapirscan, ZXing and ZBar on the same image. Try one of the examples, upload a photo, or use your camera. Images stay in your browser.

The development process was unusual too: an AI coding agent wrote the scanner algorithm. I set the goals, tested the results, and guided the experiments. Building a useful test harness became a substantial part of the work.

If you work on inventory, product lookup, or camera-based applications, I’d love to hear how it handles your images—especially the ones that are difficult to scan.

Try it: https://tapirscan.netlify.app

Code and installation: https://github.com/kleinicke/tapirscan

#OpenSource #ComputerVision #Rust

### Visual to accompany the post

Use the existing [demo screenshot](assets/demo.png) for an immediate launch. Better follow-up: record a 20–30 second screen capture using a public example, showing the same image and settings for each scanner, followed by a rotated example and a photo containing multiple symbols. Include the actual results, including failures. Label it “Example images, not an accuracy benchmark.” Do not invent results or imply one selected image establishes general superiority.

## LinkedIn: development-story follow-up

The most useful thing I built while developing a barcode scanner was the test harness.

I used an AI coding agent to write Tapirscan’s scanner algorithm. My work was setting the goals, checking labels, testing the product, and deciding what the next experiment should answer.

“Make it better” became much more useful when I could ask:

- Which missed barcodes can it read now?
- Which working cases did it break?
- Did it produce wrong values or duplicates?
- What did those gains cost in runtime?

Then I tried it on my phone. Rotation and small barcodes exposed weaknesses that my existing test images hadn’t captured well enough.

That feedback shaped the next experiments—and the released library.

I wrote up the process, including the initial neural-localizer approach and how it evolved into a classical scanner:

https://github.com/kleinicke/tapirscan/blob/main/BLOG_POST.md

## Hacker News preparation

Submit the working demo as a Show HN, with the repository available from it. Use a descriptive title containing Tapirscan, barcode scanning, and Rust/WebAssembly, prefixed with `Show HN:`. The blog is a possible later ordinary submission, not the Show HN destination.

Write the submission discussion and replies yourself. HN's current [guidelines](https://news.ycombinator.com/newsguidelines.html) prohibit generated or AI-edited comment text. These are factual preparation notes, not a comment to paste:

- Motivation: visible barcodes that existing readers missed in your tests.
- Mechanism: explicit orientation estimation and sampling across the bars.
- Scope: EAN-13, UPC-A, EAN-8 and UPC-E supported; other formats experimental; local execution.
- Your role: goals, evaluation, label checking, experiments and phone testing; disclose the agent's implementation role.
- Evidence: development observations versus a public release benchmark; explain that distinction directly.
- Useful feedback: image dimensions, expected value, selected formats, effort mode, device/browser, and a shareable reproduction.
- Tradeoff to discuss: how increased effort changes runtime; higher effort does not guarantee more reads.

The [Show HN rules](https://news.ycombinator.com/showhn.html) favor something people can immediately try and prohibit soliciting votes or comments. Choose a time when you can participate. HN also currently publishes a [temporary restriction notice](https://news.ycombinator.com/showlim) for unfamiliar/new participants; actual account eligibility has not been checked.

## Newsletter outreach

Routes checked on 16 September 2026. Editorial consideration is not guaranteed.

| Priority | Publication       | Angle                                                                                          | Submission route                                                                                                                                                                                                                                                 |
| -------- | ----------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1        | JavaScript Weekly | Browser-local WASM barcode scanning, npm package, interactive comparison                       | `editor@cooperpress.com`, listed on the publisher's [contact page](https://cooperpress.com/)                                                                                                                                                                     |
| 1        | PyCoder’s Weekly  | Native Python barcode scanning from Pillow/NumPy inputs, multiple results and geometry         | Official [link submission page](https://pycoders.com/submissions)                                                                                                                                                                                                |
| 2        | This Week in Rust | Orientation-aware scanner and lessons from shipping the Rust core through WASM/native bindings | Project updates are discovered through r/rust; direct project-update PR submissions are no longer accepted. A substantial Rust article can use the draft PR route. See [editorial guidance](https://github.com/rust-lang/this-week-in-rust/blob/main/README.md). |
| 3        | Python Weekly     | Practical image-processing library for Python                                                  | [Publication](https://www.pythonweekly.com/); editorial submission contact not verified, so hold outreach until confirmed.                                                                                                                                       |

### JavaScript Weekly pitch

Subject: Tapirscan: orientation-aware barcode scanning in WebAssembly

Hi,

I’m the creator of Tapirscan, an MIT-licensed barcode scanning library with a Rust core and a JavaScript/TypeScript package for browsers and Node.

Its current focus is EAN-13. It localizes barcode regions, estimates their orientation, and samples across the bars. Results include multiple decoded symbols and their positions. Four effort modes let applications choose how much work to spend on a frame.

The browser demo lets readers compare it with ZXing and ZBar on the same image, including their own photos, without uploading them.

Demo: https://tapirscan.netlify.app

Repository and quick start: https://github.com/kleinicke/tapirscan

Would this be a fit for JavaScript Weekly?

Florian

### PyCoder’s Weekly submission

Suggested title: Tapirscan: orientation-aware barcode scanning for Python

Link: https://github.com/kleinicke/tapirscan

Description: Tapirscan is an MIT-licensed barcode scanning library with a Rust core and native Python wheels. It accepts Pillow images, NumPy arrays and PyTorch tensors, and returns decoded symbols with source-image positions. EAN-13, UPC-A, EAN-8 and UPC-E are supported; other formats remain experimental. A browser demo provides an installation-free comparison with ZXing and ZBar: https://tapirscan.netlify.app

## Additional distribution worth doing

Prioritize one substantial technical article explaining orientation-aware scanning with real examples, using [the comparison guide](COMPARISON.md) as a starting point. This gives developers something useful to find and share beyond the initial announcement. Publish it on an owned blog or a developer publishing platform you already use; keep a canonical source.

For r/rust, prepare a technical project discussion explaining one concrete implementation tradeoff and the WASM/native packaging. This is also the project discovery route identified by This Week in Rust. Check the community's current rules before posting; do not paste the LinkedIn announcement everywhere.

Recruit a small number of relevant developers through existing relationships: people shipping inventory tools, product lookup or image pipelines. Ask them to evaluate their actual inputs. Personal feedback and an eventual integration example are more valuable at this stage than a broad paid campaign. Recipient selection and outreach remain to be done.

## Suggested first two weeks

1. Before launch: manually check the deployed demo on a phone and desktop, confirm public package installation and the blog link, and select a representative screenshot. Research here did not functionally validate the deployed demo.
2. Day 1: publish the initial LinkedIn post. Respond to actual use cases and invite reproducible examples.
3. Days 2–3: submit the JavaScript Weekly and PyCoder’s pitches. Keep the request focused on editorial coverage.
4. Days 3–5: make the Show HN submission when available to discuss it, subject to account eligibility. Link people to the project rather than asking them to boost the HN thread.
5. Days 6–8: publish the development-story LinkedIn follow-up. Incorporate concrete lessons from initial feedback if available.
6. Days 8–14: share the Rust technical discussion and work toward a public paired comparison. Report any independent integration or reproducible failure with the contributor's permission.

Track publication date, URL, replies, independently tested images, integration interest and reported defects. Review after two weeks and concentrate on channels producing useful users. Do not interpret package download counts as unique users or attribute changes to a channel without evidence.
