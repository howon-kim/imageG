<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import ImageViewer from "$lib/ImageViewer.svelte";
  import FileList from "$lib/FileList.svelte";
  import InfoPanel from "$lib/InfoPanel.svelte";
  import { bridge } from "$lib/viewerBridge";

  type ImageInfo = {
    width: number;
    height: number;
    path: string;
    exif: { tag: string; value: string }[];
    thumbnail?: string;
  };

  let imageInfo     = $state<ImageInfo | null>(null);
  let pairImageInfo = $state<ImageInfo | null>(null);
  let fileList      = $state<string[]>([]);
  let currentIndex  = $state(-1);
  let showInfo    = $state(false);
  let showFiles   = $state(true);
  let loading     = $state(false);
  let error       = $state("");
  let selectMode  = $state(false);

  const DARK_WORDS  = ["dark"];
  const LIGHT_WORDS = ["white"];

  function findPair(path: string, files: string[]): { light: string; dark: string } | null {
    if (files.length === 0) return null;
    const fileSet = new Set(files);
    const slash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
    const dir   = path.slice(0, slash + 1);
    const file  = path.slice(slash + 1);
    const dot   = file.lastIndexOf(".");
    const stem  = dot >= 0 ? file.slice(0, dot) : file;
    const ext   = dot >= 0 ? file.slice(dot)    : "";

    function trySwap(fromWords: string[], toWords: string[]): string | null {
      for (const fw of fromWords) {
        const re = new RegExp(`(^|[_\\-. ])(${fw})([_\\-. ]|$)`, "i");
        const m  = re.exec(stem);
        if (!m) continue;
        const prefix = stem.slice(0, m.index + m[1].length);
        const suffix = stem.slice(m.index + m[1].length + m[2].length);
        for (const tw of toWords) {
          const candidate = dir + prefix + tw + suffix + ext;
          if (candidate !== path && fileSet.has(candidate)) return candidate;
        }
      }
      return null;
    }

    const pairOfDark = trySwap(DARK_WORDS, LIGHT_WORDS);
    if (pairOfDark) return { light: pairOfDark, dark: path };

    const pairOfLight = trySwap(LIGHT_WORDS, DARK_WORDS);
    if (pairOfLight) return { light: path, dark: pairOfLight };

    return null;
  }

  onMount(() => {
    window.focus();
    function handleKey(e: KeyboardEvent) {
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;
      // Use e.code (physical key) — more reliable than e.key across OS/layouts
      switch (e.code) {
        case "ArrowRight": case "ArrowDown": e.preventDefault(); navigate(1); break;
        case "ArrowLeft":  case "ArrowUp":   e.preventDefault(); navigate(-1); break;
        case "KeyI": showInfo = !showInfo; break;
        case "KeyP": showFiles = !showFiles; break;
      }
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  });

  async function openFile() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp", "tiff", "tif", "bmp", "gif"] }],
    });
    window.focus();
    if (!selected) return;
    const path = typeof selected === "string" ? selected : selected[0];
    await loadImage(path);
  }

  async function openFolder() {
    const selected = await open({ directory: true });
    window.focus();
    if (!selected) return;
    const folder = typeof selected === "string" ? selected : selected[0];
    const files: string[] = await invoke("list_images", { folder });
    fileList = files;
    if (files.length > 0) { currentIndex = 0; await loadImage(files[0]); }
  }

  async function loadImage(path: string) {
    loading = true;
    error = "";
    pairImageInfo = null;
    try {
      // When no folder is open, scan the file's own directory for pair detection
      let searchFiles = fileList;
      if (searchFiles.length === 0) {
        const slash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
        const dir = path.slice(0, slash + 1) || ".";
        try { searchFiles = await invoke<string[]>("list_images", { folder: dir }); } catch { searchFiles = [path]; }
      }

      const pair = findPair(path, searchFiles);
      // Always show light on top — if user clicked the dark image, use the light as primary
      const primaryPath = pair ? pair.light : path;
      const darkPath    = pair ? pair.dark  : null;

      const info: ImageInfo = await invoke("open_image", { path: primaryPath });
      imageInfo = info;
      loading = false;

      const idx = fileList.indexOf(primaryPath);
      if (idx !== -1) currentIndex = idx;
      else { const i2 = fileList.indexOf(path); if (i2 !== -1) currentIndex = i2; }

      invoke("get_thumbnail", { path: primaryPath, maxSize: 512 }).then((thumb: any) => {
        if (imageInfo?.path === primaryPath) imageInfo = { ...imageInfo!, thumbnail: thumb };
      });

      if (darkPath) {
        invoke("open_image", { path: darkPath }).then((di: any) => {
          pairImageInfo = di as ImageInfo;
          invoke("get_thumbnail", { path: darkPath!, maxSize: 512 }).then((thumb: any) => {
            if (pairImageInfo?.path === darkPath) pairImageInfo = { ...pairImageInfo!, thumbnail: thumb as string };
          });
        }).catch(() => { pairImageInfo = null; });
      }
    } catch (e: any) {
      error = e.toString();
      loading = false;
    }
  }

  async function navigate(delta: number) {
    if (fileList.length === 0) return;
    const next = (currentIndex + delta + fileList.length) % fileList.length;
    currentIndex = next;
    await loadImage(fileList[next]);
  }
</script>

<div class="app">
  <header class="toolbar">
    <div class="toolbar-left">
      <button onclick={openFile}>Open File</button>
      <button onclick={openFolder}>Open Folder</button>
    </div>
    <div class="toolbar-center">
      {#if imageInfo}
        <span class="image-name">{imageInfo.path.split("/").pop()}</span>
        <span class="image-size">{imageInfo.width} × {imageInfo.height}</span>
        {#if fileList.length > 1}
          <span class="nav-info">{currentIndex + 1} / {fileList.length}</span>
        {/if}
      {/if}
    </div>
    <div class="toolbar-right">
      {#if imageInfo}
        <button class:active={selectMode} onclick={() => selectMode = !selectMode} title="Select Area">Area</button>
        <button onclick={() => bridge.clearSelections()} title="Clear selections">Clear</button>
        <button onclick={() => bridge.zoomOut()} title="Zoom Out (-)">−</button>
        <button onclick={() => bridge.fitToWindow()} title="Fit (F)">Fit</button>
        <button onclick={() => bridge.zoomIn()} title="Zoom In (=)">+</button>
        <button onclick={() => bridge.zoomTo(1)} title="100%">1:1</button>
      {/if}
    </div>
  </header>

  <div class="body">
    {#if showFiles && fileList.length > 0}
      <aside class="file-panel">
        <FileList files={fileList} {currentIndex} onselect={(i: number) => { currentIndex = i; loadImage(fileList[i]); }} />
      </aside>
    {/if}

    <main class="viewer-area">
      {#if loading}
        <div class="overlay"><span class="spinner"></span> Loading…</div>
      {:else if error}
        <div class="overlay error">{error}</div>
      {:else if imageInfo}
        <ImageViewer info={imageInfo} pairInfo={pairImageInfo} {selectMode} />
      {:else}
        <div class="overlay hint">
          <p>Open an image file or folder to get started.</p>
          <p class="shortcuts">← → navigate · = zoom in · - zoom out · F fit · 1 100%</p>
        </div>
      {/if}
    </main>

    {#if showInfo && imageInfo}
      <aside class="right-panels">
        <div class="panel">
          <div class="panel-title">Info</div>
          <InfoPanel info={imageInfo} />
        </div>
      </aside>
    {/if}
  </div>
</div>

<style>
  :global(*, *::before, *::after) { box-sizing: border-box; margin: 0; padding: 0; }
  :global(body) { background: #1a1a1a; color: #e0e0e0; font-family: system-ui, sans-serif; font-size: 13px; overflow: hidden; }

  .app { display: flex; flex-direction: column; height: 100vh; }
  .toolbar { display: flex; align-items: center; justify-content: space-between; background: #2a2a2a; border-bottom: 1px solid #3a3a3a; padding: 6px 12px; gap: 8px; flex-shrink: 0; }
  .toolbar-left, .toolbar-right { display: flex; gap: 6px; }
  .toolbar-center { flex: 1; text-align: center; display: flex; gap: 10px; justify-content: center; align-items: center; overflow: hidden; }
  .image-name { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 300px; }
  .image-size { color: #888; white-space: nowrap; }
  .nav-info { color: #aaa; background: #333; border-radius: 10px; padding: 1px 8px; font-size: 11px; }

  button { background: #3a3a3a; border: 1px solid #555; color: #e0e0e0; border-radius: 4px; padding: 4px 10px; cursor: pointer; font-size: 12px; }
  button:hover { background: #4a4a4a; }
  button.active { background: #2a5a9a; border-color: #4a8ad4; }

  .body { display: flex; flex: 1; overflow: hidden; }
  .file-panel { width: 180px; flex-shrink: 0; background: #232323; border-right: 1px solid #3a3a3a; overflow-y: auto; }
  .viewer-area { flex: 1; position: relative; overflow: hidden; display: flex; align-items: center; justify-content: center; background: #111; }
  .right-panels { width: 220px; flex-shrink: 0; background: #232323; border-left: 1px solid #3a3a3a; overflow-y: auto; }
  .panel { background: #1e1e1e; }
  .panel-title { padding: 6px 10px; font-size: 11px; font-weight: 600; color: #aaa; text-transform: uppercase; letter-spacing: 0.5px; border-bottom: 1px solid #333; }

  .overlay { display: flex; flex-direction: column; gap: 12px; align-items: center; justify-content: center; color: #666; text-align: center; width: 100%; height: 100%; }
  .overlay.error { color: #e05555; }
  .hint p { color: #555; }
  .shortcuts { font-size: 11px; color: #444; }

  .spinner { display: inline-block; width: 20px; height: 20px; border: 2px solid #444; border-top-color: #4a8ad4; border-radius: 50%; animation: spin 0.7s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
