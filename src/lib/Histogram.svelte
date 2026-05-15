<script lang="ts">
  import { onMount } from "svelte";

  type HistogramData = { r: number[]; g: number[]; b: number[]; luma: number[] };
  let { data }: { data: HistogramData | null } = $props();

  let canvas: HTMLCanvasElement = $state()!;
  let mode = $state<"rgb" | "luma">("rgb");

  $effect(() => {
    if (data) draw();
  });

  $effect(() => {
    mode; // track mode changes
    if (data) draw();
  });

  onMount(() => { if (data) draw(); });

  function draw() {
    if (!canvas || !data) return;
    const ctx = canvas.getContext("2d")!;
    const W = canvas.width;
    const H = canvas.height;

    ctx.clearRect(0, 0, W, H);
    ctx.fillStyle = "#181818";
    ctx.fillRect(0, 0, W, H);

    if (mode === "rgb") {
      drawChannel(ctx, data.r, "rgba(220,60,60,0.7)", W, H);
      drawChannel(ctx, data.g, "rgba(60,200,80,0.7)", W, H);
      drawChannel(ctx, data.b, "rgba(60,120,220,0.7)", W, H);
    } else {
      drawChannel(ctx, data.luma, "rgba(200,200,200,0.85)", W, H);
    }

    // Grid lines
    ctx.strokeStyle = "rgba(255,255,255,0.07)";
    ctx.lineWidth = 1;
    for (let i = 1; i < 4; i++) {
      const x = (W * i) / 4;
      ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, H); ctx.stroke();
    }
  }

  function drawChannel(ctx: CanvasRenderingContext2D, vals: number[], color: string, W: number, H: number) {
    const max = Math.max(...vals, 1);
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.moveTo(0, H);
    for (let i = 0; i < 256; i++) {
      const x = (i / 255) * W;
      const h = (vals[i] / max) * H;
      ctx.lineTo(x, H - h);
    }
    ctx.lineTo(W, H);
    ctx.closePath();
    ctx.fill();
  }
</script>

<div class="histo-wrap">
  {#if !data}
    <div class="loading">Computing…</div>
  {:else}
    <div class="mode-switch">
      <button class:active={mode === "rgb"} onclick={() => mode = "rgb"}>RGB</button>
      <button class:active={mode === "luma"} onclick={() => mode = "luma"}>Luma</button>
    </div>
    <canvas bind:this={canvas} width="200" height="100"></canvas>
    <div class="labels"><span>0</span><span>128</span><span>255</span></div>
  {/if}
</div>

<style>
  .histo-wrap { padding: 8px; display: flex; flex-direction: column; gap: 4px; }
  canvas { width: 100%; height: auto; border-radius: 3px; display: block; }
  .loading { color: #555; font-size: 11px; padding: 8px 0; }
  .labels { display: flex; justify-content: space-between; font-size: 10px; color: #555; }
  .mode-switch { display: flex; gap: 4px; }
  .mode-switch button {
    flex: 1; background: #2a2a2a; border: 1px solid #444; color: #888;
    border-radius: 3px; padding: 2px 0; font-size: 11px; cursor: pointer;
  }
  .mode-switch button.active { background: #2a5a9a; border-color: #4a8ad4; color: #fff; }
</style>
