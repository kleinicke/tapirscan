<script lang="ts">
  import { onDestroy } from "svelte";
  export let label: string;
  export let value: string;
  export let negative: string;
  export let positive: string;
  export let move: (_amount: number) => void;
  let rate = 0;
  let frame = 0;
  let lastTime = 0;
  let pointer: number | undefined;
  function stop() {
    cancelAnimationFrame(frame);
    frame = 0;
    rate = 0;
    pointer = undefined;
  }
  function advance(now: number) {
    const seconds = Math.min(0.05, Math.max(0, (now - lastTime) / 1000));
    lastTime = now;
    move(Math.sign(rate) * rate * rate * seconds);
    frame = rate ? requestAnimationFrame(advance) : 0;
  }
  function start(event: Event) {
    rate = Number((event.currentTarget as HTMLInputElement).value);
    if (!frame && rate) {
      lastTime = performance.now();
      frame = requestAnimationFrame(advance);
    }
  }
  function capture(event: PointerEvent) {
    pointer = event.pointerId;
    (event.currentTarget as HTMLInputElement).setPointerCapture(event.pointerId);
  }
  function release(event: PointerEvent) {
    if (pointer === event.pointerId) stop();
  }
  onDestroy(stop);
</script>

<svelte:window on:pointerup={release} on:pointercancel={release} on:blur={stop} />
<svelte:document on:visibilitychange={stop} />
<label>
  <span>{label}<output>{value}</output></span>
  <small>{positive}</small>
  <div class="track">
    <input
      type="range"
      min="-1"
      max="1"
      step="0.05"
      aria-label={`${label} speed`}
      aria-valuetext={`${value}; ${rate === 0 ? "stopped" : rate < 0 ? negative : positive}. Release to stop.`}
      bind:value={rate}
      on:input={start}
      on:pointerdown={capture}
      on:lostpointercapture={release}
      on:keyup={stop}
      on:blur={stop}
    />
  </div>
  <small>{negative}</small>
</label>

<style>
  label {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    width: 44px;
    box-sizing: border-box;
    padding: 5px 3px;
    background: #102724e6;
    color: white;
    border-radius: 8px;
    font-size: 10px;
    touch-action: none;
  }
  label > span,
  small {
    display: flex;
    flex-direction: column;
    gap: 1px;
    align-items: center;
  }
  output {
    font-variant-numeric: tabular-nums;
  }
  .track {
    position: relative;
  }
  .track::before {
    content: "";
    position: absolute;
    left: 3px;
    right: 3px;
    top: 50%;
    border-top: 1px solid #ffffff80;
    pointer-events: none;
  }
  input {
    position: relative;
    writing-mode: vertical-lr;
    direction: rtl;
    width: 28px;
    height: var(--adjust-track-height, 65px);
    margin: 0;
    accent-color: #b4e7ca;
    cursor: pointer;
    touch-action: none;
  }
  small {
    font-size: 10px;
    opacity: 0.8;
  }
</style>
