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
  let selectMode   = $state(false);
  let showDiff     = $state(true);
  let sidebarWidth = $state(180);

  function startResize(e: MouseEvent) {
    const startX = e.clientX, startW = sidebarWidth;
    function onMove(e: MouseEvent) {
      sidebarWidth = Math.max(120, Math.min(480, startW + e.clientX - startX));
    }
    function onUp() {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    }
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    e.preventDefault();
  }

  type SidebarItem = { path: string; origIdx: number; hasPair: boolean };

  function computeSidebarItems(files: string[]): SidebarItem[] {
    const darkPaths = new Set<string>();
    const hasPairIdx = new Set<number>();
    for (let i = 0; i < files.length; i++) {
      const pair = findPair(files[i], files);
      if (pair) { darkPaths.add(pair.dark); hasPairIdx.add(i); }
    }
    return files
      .map((f, i) => ({ path: f, origIdx: i, hasPair: hasPairIdx.has(i) }))
      .filter(item => !darkPaths.has(item.path));
  }

  let sidebarItems = $derived(computeSidebarItems(fileList));

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
      {#if fileList.length > 0}
        <button class:active={showFiles} onclick={() => showFiles = !showFiles} title="Toggle sidebar (P)">☰</button>
      {/if}
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
        <button class:active={showDiff} onclick={() => showDiff = !showDiff} title="Toggle pixel diff heatmap">Diff</button>
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
      <aside class="file-panel" style="width: {sidebarWidth}px">
        <div class="file-scroll">
          <FileList items={sidebarItems} {currentIndex} onselect={(origIdx: number) => { currentIndex = origIdx; loadImage(fileList[origIdx]); }} />
        </div>
        <div class="resize-handle" role="separator" aria-orientation="vertical" onmousedown={startResize}></div>
      </aside>
    {/if}

    <main class="viewer-area">
      {#if loading}
        <div class="overlay"><span class="spinner"></span> Loading…</div>
      {:else if error}
        <div class="overlay error">{error}</div>
      {:else if imageInfo}
        <ImageViewer info={imageInfo} pairInfo={pairImageInfo} {selectMode} {showDiff} />
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
  :global(body) { background: #0e0e10; color: #d0d0e0; font-family: system-ui, sans-serif; font-size: 13px; overflow: hidden; }

  .app { display: flex; flex-direction: column; height: 100vh; }
  .toolbar {
    display: flex; align-items: center; justify-content: space-between;
    background: #161618; border-bottom: 1px solid #24242c;
    padding: 6px 12px; gap: 8px; flex-shrink: 0;
  }
  .toolbar-left, .toolbar-right { display: flex; gap: 5px; align-items: center; }
  .toolbar-center { flex: 1; text-align: center; display: flex; gap: 10px; justify-content: center; align-items: center; overflow: hidden; }
  .image-name { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 300px; color: #c0c0d8; }
  .image-size { color: #585870; white-space: nowrap; font-size: 12px; }
  .nav-info { color: #707088; background: #1a1a22; border: 1px solid #28283a; border-radius: 10px; padding: 1px 8px; font-size: 11px; }

  button {
    background: #1c1c22; border: 1px solid #2e2e3a; color: #9898b4;
    border-radius: 5px; padding: 4px 10px; cursor: pointer; font-size: 12px;
    transition: background 0.1s, border-color 0.1s, color 0.1s;
  }
  button:hover { background: #242430; border-color: #38384a; color: #c0c0d8; }
  button.active { background: #142050; border-color: #3460c0; color: #90b4ff; }

  .body { display: flex; flex: 1; overflow: hidden; }
  .file-panel {
    position: relative; flex-shrink: 0;
    background: #101012; border-right: 1px solid #202028;
    display: flex; flex-direction: column; overflow: hidden;
  }
  .file-scroll { flex: 1; overflow-y: auto; }
  .file-scroll::-webkit-scrollbar { width: 4px; }
  .file-scroll::-webkit-scrollbar-track { background: transparent; }
  .file-scroll::-webkit-scrollbar-thumb { background: #28283a; border-radius: 2px; }
  .resize-handle {
    position: absolute; right: 0; top: 0; bottom: 0; width: 4px;
    cursor: col-resize; z-index: 10; background: transparent;
  }
  .resize-handle:hover { background: rgba(91,142,240,0.3); }
  .viewer-area { flex: 1; position: relative; overflow: hidden; display: flex; align-items: center; justify-content: center; background: #0b0b0e; }
  .right-panels { width: 220px; flex-shrink: 0; background: #101012; border-left: 1px solid #202028; overflow-y: auto; }
  .panel { background: #101012; }
  .panel-title { padding: 6px 10px; font-size: 10px; font-weight: 700; color: #505060; text-transform: uppercase; letter-spacing: 0.8px; border-bottom: 1px solid #1c1c24; }

  .overlay { display: flex; flex-direction: column; gap: 12px; align-items: center; justify-content: center; color: #383848; text-align: center; width: 100%; height: 100%; }
  .overlay.error { color: #b04040; }
  .hint p { color: #363645; }
  .shortcuts { font-size: 11px; color: #282838; }

  .spinner { display: inline-block; width: 20px; height: 20px; border: 2px solid #24242e; border-top-color: #5b8ef0; border-radius: 50%; animation: spin 0.7s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
