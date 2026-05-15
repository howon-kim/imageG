<script lang="ts">
  type ImageInfo = {
    width: number; height: number; path: string;
    exif: { tag: string; value: string }[];
  };
  let { info }: { info: ImageInfo } = $props();

  function filename(path: string) { return path.split("/").pop() ?? path; }
  function filesize(path: string) { return ""; } // Tauri fs can provide this if needed
</script>

<div class="info-panel">
  <div class="row"><span class="label">File</span><span class="value">{filename(info.path)}</span></div>
  <div class="row"><span class="label">Size</span><span class="value">{info.width} × {info.height} px</span></div>
  <div class="row"><span class="label">Megapixels</span><span class="value">{((info.width * info.height) / 1_000_000).toFixed(1)} MP</span></div>
  {#if info.exif.length > 0}
    <div class="section-title">EXIF</div>
    {#each info.exif.slice(0, 20) as entry}
      <div class="row">
        <span class="label">{entry.tag}</span>
        <span class="value">{entry.value}</span>
      </div>
    {/each}
  {/if}
</div>

<style>
  .info-panel { padding: 8px; display: flex; flex-direction: column; gap: 3px; }
  .row { display: flex; gap: 6px; align-items: baseline; }
  .label { color: #666; font-size: 11px; min-width: 70px; flex-shrink: 0; }
  .value { color: #ccc; font-size: 11px; word-break: break-all; }
  .section-title { color: #666; font-size: 10px; text-transform: uppercase; letter-spacing: 0.5px; margin-top: 6px; padding-top: 6px; border-top: 1px solid #333; }
</style>
