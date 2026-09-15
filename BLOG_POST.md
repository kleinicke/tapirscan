# How I used an AI coding agent to build Tapirscan

[Tapirscan](README.md) is a barcode library with a [browser demo](https://tapirscan.netlify.app). I built Tapirscan with the help of GPT-6 Astra. Astra wrote the code; I set the goals, tested the results, and guided the experiments. This is the story of that development; the comparisons below describe research observations, rather than a published benchmark of the release.

This started with a problem, before I had a plan to build a library. Open-source barcode scanners seemed worse than they needed to be. I could clearly see a barcode in an image, yet the scanner returned nothing. Why?

My first guess was localization: perhaps the scanner simply wasn’t finding the barcode. So I built a neural-network-based localizer. That helped a lot, but it wasn’t perfect. I was preparing to collect and label a large dataset myself to improve it further.

Before investing that time, I asked Astra to optimize everything around the localizer and build a new pipeline in Rust. My requirements were straightforward: fast, versatile, and robust. But the most important work was **building the test harness that would tell me whether the scanner was actually getting there**.

That harness was a substantial part of the project. I needed a collection of test images, checked labels, repeatable scanner runs, and a consistent way to score the outputs. ZXing and ZBar became ongoing reference points. The scanner had to read the correct value at the correct location, handle multiple barcodes, and report wrong reads and duplicates as well as successes. The harness measured localization, decoding, and total runtime separately.

This made the task much more concrete than “keep improving the scanner.” For each candidate, I could ask: which previously missed barcodes does it read now? Which working cases did it break? Did it become slower? And was a failure caused by localization or decoding? The harness gave the agent feedback it could act on and gave me results I could inspect without reading the implementation.

I asked Astra to build the development loop around that feedback: implement a change, run focused checks, compare it with the previous version, and expand to the broader benchmark when the result justified it. Saved ZXing and ZBar outputs could be reused when the images and scanner settings stayed the same. New inputs needed new reference runs, and speed claims needed fresh, comparable timing measurements. Every small edit didn’t require rerunning every scanner.

Eventually, the new pipeline had its own classical localizer and decoder. I asked Astra to optimize those further, with neural localization remaining an option. The harness made it possible to explore that change in direction while checking what each change gained and lost.

## The first result that made me believe in it

It was slow at first, and I got frustrated. Surely building something as good as the available open-source libraries couldn’t be _that_ hard? I had underestimated how much refinement went into them. Sean Owen [announced the original Java ZXing project in November 2007](https://developers.googleblog.com/zxing-1d2d-barcode-decoding-source-code-released/); my benchmark uses the separately maintained ZXing-C++ through `zxing-wasm`.

Their EAN-13 readers use horizontal and vertical scanlines to look for a decodable pattern. ZXing-C++ searches image rows, while ZBar can assemble compatible half-symbol reads from different scanlines. Tapirscan developed an explicit orientation-estimation step so it could sample along the barcode. These are different approaches to finding useful evidence, and matching established implementations took more work than I expected. The [technical comparison](docs/COMPARISON.md) explains the differences and their limits.

Then came a result that changed how I felt about the project. The ongoing evaluations used my combined private and public EAN-13 development sets, rather than a separate untouched holdout. At one milestone, Astra reported that Tapirscan read 13 images ZXing missed—and ZXing read 13 that Tapirscan missed.

That wasn’t a victory, but it was the first concrete sign that I had something worth developing. The two approaches had different strengths, beyond simply matching an existing implementation. There were specific failures to investigate and specific gains to protect. I told it to continue.

I also challenged the development process itself: why were the iterations taking so long? I asked Astra to improve the experiment loop, including how changes were screened before broader evaluation. This was mostly an implementation-and-evaluation loop, not repeated neural-network training. **Improving how the agent worked was part of improving the scanner.**

Later reports were encouraging, but a development comparison is not a general ranking of libraries. ZXing and ZBar remained reference points throughout the work. I wanted to understand which inputs each approach handled, where it failed, and what those gains cost in runtime. A public performance claim needs a reproducible comparison of the actual release; the [benchmark guide](docs/BENCHMARKS.md) describes how to evaluate that.

## Why I asked for Rust

In my experience, Rust is a useful choice for this kind of compute-heavy work. I wanted a fast, versatile, robust core that could also run in the browser through WebAssembly. **You don’t need to know how to write Rust to ask the AI to use it.** You do need to explain the outcome you want and have a way to check it.

When I ask an AI to move a working program to Rust, it often questions the motivation: translating the same algorithm won’t automatically make it faster. That’s a fair point. What I’ve found useful is combining the rewrite with optimization—giving the AI room to reconsider the algorithms, memory use, and repeated computation, then measuring the result. For compute-heavy components such as this scanner, that can be a productive approach. Moving UI code to Rust wouldn’t address the same bottlenecks. The demo uses TypeScript and Svelte for its UI.

## The phone showed what my tests were missing

Throughout the process, I inspected failure cases and corrected labels. Some images were damaged; others had such low resolution that I wasn’t sure whether the barcode could be decoded at all. Any exclusions needed to stay explicit and documented. Removing a difficult image can improve a score without improving the algorithm.

Once the benchmark results looked promising, I asked the agent to build a small website. I wanted to compare the classical pipeline, the neural-localizer variant, ZXing, and ZBar using my phone’s camera.

That was essential. In the live demo, I found low-resolution barcodes that still caused problems even when the region was found. Rotation was another major weakness, including for ZXing and ZBar in the demo. Good results on my existing test images hadn’t captured enough of what mattered in actual use.

I asked Astra to focus on those cases and turn the observations into controlled rotation and resolution experiments, including real photographs rotated in steps. Those experiments shaped the released version of Tapirscan. They also gave me concrete weaknesses to keep investigating as people try it on images and devices I haven’t tested.

## What the builder actually needs

**You don’t need to be able to read or understand the code to lead this kind of development.** You do need a vision concrete enough to test: who is this for, what should work better, and what would count as success? For me, that meant finding more barcodes, making fewer wrong reads, and staying fast on a phone.

Vision alone isn’t enough. You need curiosity, judgment, and a willingness to try the product and challenge the answers. “This barcode works until I rotate the phone” can be extremely useful feedback. So can “the benchmark looks good, but the live readout is unusable.” Ask the agent to explain tradeoffs in plain language and provide evidence for its claims. Technical understanding helps, and experienced engineering review still matters before calling a library production-ready.

To repeat this approach:

1. Start with a problem you can observe and a concrete definition of better.
2. Invest in a reproducible test harness, checked labels, and fair competing baselines.
3. Compare individual successes and failures, not just aggregate scores.
4. Let the agent implement and experiment, but challenge its conclusions and workflow.
5. Try promising candidates in the real product early.
6. Turn discoveries into regression tests, while preserving an untouched final test set.

Long autonomous runs helped, but **clear measurements and useful feedback mattered more than simply giving the agent more time**. My initial localization hypothesis provided a starting point. The harness showed me where the scanner was improving. Using it on my phone showed me what to ask Astra to work on next.

[Try Tapirscan in your browser](https://tapirscan.netlify.app), or see the [README](README.md) for installation and current format coverage.
