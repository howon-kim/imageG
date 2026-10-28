<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { bridge } from "$lib/viewerBridge";

  type ImageInfo = { width: number; height: number; path: string; thumbnail?: string };
  type AreaSel  = { id: number; x: number; y: number; w: number; h: number; avgGV: number | null };
  let { info, pairInfo = null, selectMode = false, showDiff = true }: { info: ImageInfo; pairInfo?: ImageInfo | null; selectMode?: boolean; showDiff?: boolean } = $props();

  let canvas: HTMLCanvasElement;
  let container: HTMLDivElement;
  let ctx: CanvasRenderingContext2D;

  let zoom      = $state(1);
  let offsetX   = $state(0);
  let offsetY   = $state(0);
  let splitRatio = $state(0.5);
  let viewMode   = $state<"light" | "both" | "dark">("both");
  let selections = $state<AreaSel[]>([]);
  let nextSelId  = 0;
  let selDraftStart: { x: number; y: number } | null = null;
  let selDraftEnd:   { x: number; y: number } | null = null;
  let selHits: Array<{ id: number; cx: number; cy: number }> = [];

  let rafId            = 0;
  let lastZoomTime     = 0;
  let tilesFetchTimer  = 0;

  let dragging       = false;
  let splitDragging  = false;
  let minimapDragging = false;
  let dragStart = { x: 0, y: 0, ox: 0, oy: 0 };
  let mmapBx = 0, mmapBy = 0, mmapMw = 0, mmapMh = 0;

  const TILE_SIZE  = 512;
  const RULER_SIZE = 20;

  // Primary image state
  let tileCache    = new Map<string, ImageBitmap>();
  let loadingTiles = new Set<string>();
  let thumbImg:    ImageBitmap | null = null;
  let thumbPath  = "";
  let prevPath   = "";

  // Pair image state
  let pairTileCache    = new Map<string, ImageBitmap>();
  let pairLoadingTiles = new Set<string>();
  let pairThumbImg:    ImageBitmap | null = null;
  let pairThumbPath  = "";
  let prevPairPath   = "";

  // Reset primary when path changes
  $effect(() => {
    const path = info?.path ?? "";
    if (!path || path === prevPath) return;
    prevPath = path;
    tileCache.clear(); loadingTiles.clear();
    thumbImg = null; thumbPath = "";
    selections = [];
    fitToWindow();
  });

  // Load primary thumbnail
  $effect(() => {
    const t = info?.thumbnail, p = info?.path;
    if (!t || !p || p === thumbPath) return;
    b64ToBlob(t).then(blob => createImageBitmap(blob)).then(bmp => {
      if (info?.path === p) { thumbImg = bmp; thumbPath = p; scheduleDraw(); }
    });
  });

  // Reset pair state when pair path changes
  $effect(() => {
    const path = pairInfo?.path ?? "";
    if (path === prevPairPath) return;
    prevPairPath = path;
    pairTileCache.clear(); pairLoadingTiles.clear();
    pairThumbImg = null; pairThumbPath = "";
    if (!path) viewMode = "both";
    draw();
    if (path) fetchVisibleTiles();
  });

  // Load pair thumbnail
  $effect(() => {
    const t = pairInfo?.thumbnail, p = pairInfo?.path;
    if (!t || !p || p === pairThumbPath) return;
    b64ToBlob(t).then(blob => createImageBitmap(blob)).then(bmp => {
      if (pairInfo?.path === p) { pairThumbImg = bmp; pairThumbPath = p; scheduleDraw(); }
    });
  });

  onMount(() => {
    ctx = canvas.getContext("2d")!;
    const ro = new ResizeObserver(() => fitToWindow());
    ro.observe(container);
    fitToWindow();

    bridge.zoomIn        = () => setZoom(zoom * 1.25);
    bridge.zoomOut       = () => setZoom(zoom * 0.8);
    bridge.fitToWindow   = fitToWindow;
    bridge.zoomTo        = (n: number) => setZoom(n);
    bridge.clearSelections = () => { selections = []; draw(); };

    function handleKey(e: KeyboardEvent) {
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;
      switch (e.code) {
        case "Equal":  setZoom(zoom * 1.25); break;
        case "Minus":  setZoom(zoom * 0.8);  break;
        case "KeyF":   fitToWindow();         break;
        case "Digit1": setZoom(1);            break;
        case "Digit2": setZoom(2);            break;
      }
    }
    document.addEventListener("keydown", handleKey);
    return () => {
      ro.disconnect();
      document.removeEventListener("keydown", handleKey);
      bridge.zoomIn = bridge.zoomOut = bridge.fitToWindow = () => {};
      bridge.zoomTo = bridge.clearSelections = () => {};
    };
  });

  function fitToWindow() {
    if (!ctx || !container || !info) return;
    const cw = container.offsetWidth, ch = container.offsetHeight;
    if (!cw || !ch) return;
    zoom    = Math.min(cw / info.width, ch / info.height);
    offsetX = (cw - info.width  * zoom) / 2;
    offsetY = (ch - info.height * zoom) / 2;
    tileCache.clear(); loadingTiles.clear();
    pairTileCache.clear(); pairLoadingTiles.clear();
    draw();
    fetchVisibleTiles();
  }

  function setZoom(newZoom: number, pivotX?: number, pivotY?: number) {
    if (!ctx || !container || !info) return;
    const cw = container.offsetWidth, ch = container.offsetHeight;
    if (!cw || !ch) return;
    const px = pivotX ?? cw / 2, py = pivotY ?? ch / 2;
    const minZoom = Math.min(cw / info.width, ch / info.height) * 0.1;
    newZoom = Math.max(minZoom, Math.min(32, newZoom));
    offsetX = px - (px - offsetX) * (newZoom / zoom);
    offsetY = py - (py - offsetY) * (newZoom / zoom);
    zoom    = newZoom;
    lastZoomTime = Date.now();
    scheduleDraw();
    deferFetchTiles();
  }

  function scheduleDraw() {
    if (rafId) return;
    rafId = requestAnimationFrame(() => { rafId = 0; draw(); });
  }

  function deferFetchTiles() {
    clearTimeout(tilesFetchTimer);
    tilesFetchTimer = setTimeout(fetchVisibleTiles, 120);
  }

  // ── Drawing ──────────────────────────────────────────────────────────────

  function draw() {
    if (!ctx || !canvas || !info || !container) return;
    const cw = container.offsetWidth, ch = container.offsetHeight;
    if (!cw || !ch) return;

    // Only reset canvas when dimensions actually change — resize clears context state
    if (canvas.width !== cw || canvas.height !== ch) {
      canvas.width = cw; canvas.height = ch;
    }
    ctx.imageSmoothingEnabled = zoom < 3;
    ctx.fillStyle = "#0b0b0e";
    ctx.fillRect(0, 0, cw, ch);

    if (pairInfo && viewMode === "both") {
      const sy = Math.round(ch * splitRatio);
      ctx.save();
      ctx.beginPath(); ctx.rect(0, 0, cw, sy); ctx.clip();
      drawLayer(info, thumbImg, tileCache);
      ctx.restore();
      ctx.save();
      ctx.beginPath(); ctx.rect(0, sy, cw, ch - sy); ctx.clip();
      drawLayer(pairInfo, pairThumbImg, pairTileCache);
      ctx.restore();
      drawSplitLine(sy, cw);
    } else if (pairInfo && viewMode === "dark") {
      drawLayer(pairInfo, pairThumbImg, pairTileCache);
    } else {
      drawLayer(info, thumbImg, tileCache);
    }

    const zooming = Date.now() - lastZoomTime < 150;
    const gvSnap  = !zooming && zoom >= 16 ? capturePixelGV() : null;
    if (!zooming && zoom >= 6)  drawPixelGrid();
    if (gvSnap)                 drawGVLabels(gvSnap);
    if (showDiff && zoom >= 20 && gvSnap) drawPixelDiffs(gvSnap);
    drawSelections();
    if (zoom >= 1)  drawRuler();
    drawMinimap();
  }

  function drawLayer(imgInfo: ImageInfo, thumb: HTMLImageElement | null, tiles: Map<string, HTMLImageElement>) {
    if (thumb) {
      ctx.drawImage(thumb, offsetX, offsetY, imgInfo.width * zoom, imgInfo.height * zoom);
    }
    const cw = canvas.width, ch = canvas.height;
    const tileScale = Math.min(zoom, 1);
    const imgL = Math.max(0, Math.floor(-offsetX / zoom));
    const imgT = Math.max(0, Math.floor(-offsetY / zoom));
    const imgR = Math.min(imgInfo.width,  Math.ceil((cw - offsetX) / zoom));
    const imgB = Math.min(imgInfo.height, Math.ceil((ch - offsetY) / zoom));

    for (let ty = Math.floor(imgT / TILE_SIZE); ty < Math.ceil(imgB / TILE_SIZE); ty++) {
      for (let tx = Math.floor(imgL / TILE_SIZE); tx < Math.ceil(imgR / TILE_SIZE); tx++) {
        const sx = tx * TILE_SIZE, sy = ty * TILE_SIZE;
        const sw = Math.min(TILE_SIZE, imgInfo.width  - sx);
        const sh = Math.min(TILE_SIZE, imgInfo.height - sy);
        if (sw <= 0 || sh <= 0) continue;
        const key  = `${sx},${sy},${sw},${sh},${tileScale.toFixed(3)}`;
        const tile = tiles.get(key);
        if (tile) ctx.drawImage(tile, offsetX + sx * zoom, offsetY + sy * zoom, sw * zoom, sh * zoom);
      }
    }
  }

  function drawSplitLine(sy: number, cw: number) {
    // Dashed divider
    ctx.save();
    ctx.strokeStyle = "rgba(255,255,255,0.35)";
    ctx.lineWidth = 1;
    ctx.setLineDash([6, 5]);
    ctx.beginPath(); ctx.moveTo(0, sy); ctx.lineTo(cw, sy);
    ctx.stroke();
    ctx.setLineDash([]);

    // Labels
    ctx.font = "bold 11px system-ui, sans-serif";
    ctx.textBaseline = "bottom";
    ctx.textAlign = "right";
    const lightName = info.path.split("/").pop() ?? "";
    const darkName  = pairInfo!.path.split("/").pop() ?? "";
    ctx.fillStyle = "rgba(255,255,255,0.45)";
    ctx.fillText(lightName, cw - 8, sy - 4);
    ctx.textBaseline = "top";
    ctx.fillText(darkName, cw - 8, sy + 4);

    // Drag handle
    const hx = cw / 2, r = 12;
    ctx.beginPath(); ctx.arc(hx, sy, r, 0, Math.PI * 2);
    ctx.fillStyle = "rgba(40,40,40,0.85)";
    ctx.fill();
    ctx.strokeStyle = "rgba(255,255,255,0.4)";
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.fillStyle = "rgba(255,255,255,0.6)";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.font = "10px system-ui";
    ctx.fillText("⇅", hx, sy);
    ctx.restore();
  }

  // ── Pixel overlays ───────────────────────────────────────────────────────

  type GVSnap = { data: Uint8ClampedArray; sx: number; sy: number; sw: number; sh: number; imgL: number; imgT: number; imgR: number; imgB: number };

  function capturePixelGV(): GVSnap | null {
    const cw = canvas.width, ch = canvas.height;
    const imgL = Math.max(0, Math.floor(-offsetX / zoom));
    const imgT = Math.max(0, Math.floor(-offsetY / zoom));
    const imgR = Math.min(info.width,  Math.ceil((cw - offsetX) / zoom));
    const imgB = Math.min(info.height, Math.ceil((ch - offsetY) / zoom));
    if (imgR <= imgL || imgB <= imgT) return null;

    const sx = Math.max(RULER_SIZE, Math.round(offsetX + imgL * zoom));
    const sy = Math.max(RULER_SIZE, Math.round(offsetY + imgT * zoom));
    const sw = Math.min(cw, Math.round(offsetX + imgR * zoom)) - sx;
    const sh = Math.min(ch, Math.round(offsetY + imgB * zoom)) - sy;
    if (sw <= 0 || sh <= 0) return null;

    const { data } = ctx.getImageData(sx, sy, sw, sh);
    return { data, sx, sy, sw, sh, imgL, imgT, imgR, imgB };
  }

  function drawGVLabels({ data, sx, sy, sw, sh, imgL, imgT, imgR, imgB }: GVSnap) {
    const cw = canvas.width, ch = canvas.height;
    const fontSize = Math.min(zoom * 0.38, 12);
    ctx.font = `${fontSize}px monospace`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";

    for (let iy = imgT; iy < imgB; iy++) {
      for (let ix = imgL; ix < imgR; ix++) {
        const cx = Math.round(offsetX + (ix + 0.5) * zoom);
        const cy = Math.round(offsetY + (iy + 0.5) * zoom);
        if (cx < RULER_SIZE || cy < RULER_SIZE || cx >= cw || cy >= ch) continue;
        const px = cx - sx, py = cy - sy;
        if (px < 0 || py < 0 || px >= sw || py >= sh) continue;
        const i  = (py * sw + px) * 4;
        const gv = Math.round(0.299 * data[i] + 0.587 * data[i + 1] + 0.114 * data[i + 2]);
        ctx.fillStyle = gv > 128 ? "rgba(0,0,0,0.85)" : "rgba(255,255,255,0.85)";
        ctx.fillText(String(gv), cx, cy);
      }
    }
  }

  function drawPixelDiffs({ data, sx: snapX, sy: snapY, sw, sh, imgL, imgT, imgR, imgB }: GVSnap) {
    const cw = canvas.width, ch = canvas.height;

    const MAX_C = 16;
    const midC  = (imgL + imgR) >> 1;
    const c0 = Math.max(imgL, midC - (MAX_C >> 1));
    const c1 = Math.min(imgR, c0 + MAX_C);
    const r0 = imgT, r1 = imgB;
    const cols = c1 - c0, rows = r1 - r0;
    if (cols <= 0 || rows <= 0) return;

    const gv = new Int16Array(cols * rows).fill(-1);
    for (let iy = r0; iy < r1; iy++) {
      for (let ix = c0; ix < c1; ix++) {
        const cx = Math.round(offsetX + (ix + 0.5) * zoom);
        const cy = Math.round(offsetY + (iy + 0.5) * zoom);
        const px = cx - snapX, py = cy - snapY;
        if (px < 0 || py < 0 || px >= sw || py >= sh) continue;
        const bi = (py * sw + px) * 4;
        gv[(iy - r0) * cols + (ix - c0)] =
          Math.round(0.299 * data[bi] + 0.587 * data[bi + 1] + 0.114 * data[bi + 2]);
      }
    }

    const barW    = Math.max(3, Math.min(zoom * 0.14, 6));
    const showNum = zoom >= 28;
    if (showNum) {
      ctx.font = `bold ${Math.min(zoom * 0.18, 8)}px monospace`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
    }

    for (let iy = r0; iy < r1; iy++) {
      for (let ix = c0; ix < c1 - 1; ix++) {
        const g1 = gv[(iy - r0) * cols + (ix - c0)];
        const g2 = gv[(iy - r0) * cols + (ix - c0 + 1)];
        if (g1 < 0 || g2 < 0) continue;
        const diff = Math.abs(g1 - g2);
        if (diff < 4) continue;

        const bx    = Math.round(offsetX + (ix + 1) * zoom);
        const byTop = Math.round(offsetY + iy * zoom);
        const byBot = Math.round(offsetY + (iy + 1) * zoom);
        if (bx < RULER_SIZE || bx >= cw || byBot < RULER_SIZE) continue;

        ctx.fillStyle = diffHeatColor(diff);
        ctx.fillRect(bx - barW / 2, byTop, barW, byBot - byTop);

        if (showNum) {
          ctx.fillStyle = diff > 80 ? "#fff" : "#000";
          ctx.fillText(String(diff), bx, (byTop + byBot) / 2);
        }
      }
    }
  }

  function diffHeatColor(diff: number): string {
    if (diff < 16)  return "rgba(80,160,255,0.65)";
    if (diff < 40)  return "rgba(60,210,80,0.78)";
    if (diff < 80)  return "rgba(240,210,0,0.85)";
    if (diff < 140) return "rgba(255,120,0,0.90)";
    return "rgba(255,45,45,0.95)";
  }

  function canvasToImg(cx: number, cy: number) {
    return { x: (cx - offsetX) / zoom, y: (cy - offsetY) / zoom };
  }

  function drawSelections() {
    selHits = [];
    if (selections.length === 0 && !selDraftStart) return;
    ctx.save();

    // Draft rectangle while dragging
    if (selDraftStart && selDraftEnd) {
      const x = Math.min(selDraftStart.x, selDraftEnd.x);
      const y = Math.min(selDraftStart.y, selDraftEnd.y);
      const w = Math.abs(selDraftEnd.x - selDraftStart.x);
      const h = Math.abs(selDraftEnd.y - selDraftStart.y);
      const sx = offsetX + x * zoom, sy = offsetY + y * zoom;
      const sw = w * zoom, sh = h * zoom;
      ctx.fillStyle = "rgba(78,180,255,0.08)";
      ctx.fillRect(sx, sy, sw, sh);
      ctx.strokeStyle = "rgba(78,180,255,0.7)";
      ctx.lineWidth = 1; ctx.setLineDash([4, 3]);
      ctx.strokeRect(sx + 0.5, sy + 0.5, sw, sh);
      ctx.setLineDash([]);
      const lbl = `${Math.round(w)} × ${Math.round(h)} px`;
      ctx.font = "bold 11px system-ui"; ctx.textAlign = "center"; ctx.textBaseline = "middle";
      const tw = ctx.measureText(lbl).width;
      const lx = sx + sw / 2, ly = sy + sh / 2;
      ctx.fillStyle = "rgba(0,0,0,0.6)"; ctx.fillRect(lx - tw/2 - 5, ly - 9, tw + 10, 18);
      ctx.fillStyle = "#78b8ff"; ctx.fillText(lbl, lx, ly);
    }

    const C   = "#50e898";          // accent colour
    const DIM = 16;                 // px offset from box to dimension line
    const TK  = 5;                  // extension tick half-length
    const AH  = 6;                  // arrowhead size

    function arrowHead(tx: number, ty: number, dx: number, dy: number) {
      const px = -dy, py = dx; // perpendicular
      ctx.beginPath();
      ctx.moveTo(tx, ty);
      ctx.lineTo(tx + dx * AH + px * AH * 0.35, ty + dy * AH + py * AH * 0.35);
      ctx.lineTo(tx + dx * AH - px * AH * 0.35, ty + dy * AH - py * AH * 0.35);
      ctx.closePath(); ctx.fill();
    }

    function dimLabel(text: string, x: number, y: number, rotate: boolean) {
      ctx.save();
      ctx.translate(x, y);
      if (rotate) ctx.rotate(-Math.PI / 2);
      ctx.font = "bold 10px system-ui";
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      const tw = ctx.measureText(text).width;
      ctx.fillStyle = "rgba(0,0,0,0.55)"; ctx.fillRect(-tw/2 - 3, -8, tw + 6, 16);
      ctx.fillStyle = C; ctx.fillText(text, 0, 0.5);
      ctx.restore();
    }

    for (let i = 0; i < selections.length; i++) {
      const sel = selections[i];
      const sx = offsetX + sel.x * zoom, sy = offsetY + sel.y * zoom;
      const sw = sel.w * zoom,           sh = sel.h * zoom;
      const cw = canvas.width;

      // ── Box ──────────────────────────────────────────────────────────────
      ctx.fillStyle = "rgba(80,232,152,0.06)";
      ctx.fillRect(sx, sy, sw, sh);

      ctx.strokeStyle = C; ctx.lineWidth = 1.5; ctx.setLineDash([]);
      ctx.strokeRect(sx + 0.5, sy + 0.5, sw, sh);

      // Corner handles
      const HS = 4;
      ctx.fillStyle = C;
      for (const [cx2, cy2] of [[sx,sy],[sx+sw,sy],[sx,sy+sh],[sx+sw,sy+sh]] as [number,number][]) {
        ctx.fillRect(cx2 - HS, cy2 - HS, HS * 2, HS * 2);
      }

      // ── Center: avg GV ───────────────────────────────────────────────────
      const avgLbl = sel.avgGV !== null ? `⌀ ${sel.avgGV.toFixed(1)}` : "…";
      const lx = sx + sw / 2, ly = sy + sh / 2;
      ctx.font = "bold 13px system-ui"; ctx.textAlign = "center"; ctx.textBaseline = "middle";
      const aw = ctx.measureText(avgLbl).width;
      ctx.fillStyle = "rgba(0,0,0,0.68)"; ctx.fillRect(lx - aw/2 - 6, ly - 11, aw + 12, 22);
      ctx.fillStyle = "#fff"; ctx.fillText(avgLbl, lx, ly);

      // ── Index badge ──────────────────────────────────────────────────────
      ctx.font = "bold 9px system-ui"; ctx.textAlign = "left"; ctx.textBaseline = "top";
      const badge = `#${i + 1}`;
      const bw = ctx.measureText(badge).width;
      ctx.fillStyle = "rgba(0,0,0,0.5)"; ctx.fillRect(sx + 4, sy + 4, bw + 6, 13);
      ctx.fillStyle = C; ctx.fillText(badge, sx + 7, sy + 5);

      // ── × delete ─────────────────────────────────────────────────────────
      const hx = Math.min(sx + sw, cw - 4) - 10;
      const hy = Math.max(sy, RULER_SIZE + 4) + 10;
      selHits.push({ id: sel.id, cx: hx, cy: hy });
      ctx.beginPath(); ctx.arc(hx, hy, 7, 0, Math.PI * 2);
      ctx.fillStyle = "rgba(220,60,60,0.88)"; ctx.fill();
      ctx.font = "bold 11px system-ui"; ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.fillStyle = "#fff"; ctx.fillText("×", hx, hy + 0.5);

      // ── Dimension arrows ─────────────────────────────────────────────────
      ctx.strokeStyle = C; ctx.fillStyle = C; ctx.lineWidth = 1;

      // Width arrow — below box
      const wy  = sy + sh + DIM;
      const wmx = sx + sw / 2;
      // Extension lines
      ctx.beginPath();
      ctx.moveTo(sx,      sy + sh); ctx.lineTo(sx,      wy + TK);
      ctx.moveTo(sx + sw, sy + sh); ctx.lineTo(sx + sw, wy + TK);
      ctx.stroke();
      // Dimension line
      ctx.beginPath(); ctx.moveTo(sx, wy); ctx.lineTo(sx + sw, wy); ctx.stroke();
      // Arrowheads pointing inward
      arrowHead(sx,      wy, +1,  0);
      arrowHead(sx + sw, wy, -1,  0);
      // Label
      dimLabel(`${sel.w} px`, wmx, wy, false);

      // Height arrow — right of box
      const hax = sx + sw + DIM;
      const hmy = sy + sh / 2;
      // Extension lines
      ctx.beginPath();
      ctx.moveTo(sx + sw, sy);      ctx.lineTo(hax + TK, sy);
      ctx.moveTo(sx + sw, sy + sh); ctx.lineTo(hax + TK, sy + sh);
      ctx.stroke();
      // Dimension line
      ctx.beginPath(); ctx.moveTo(hax, sy); ctx.lineTo(hax, sy + sh); ctx.stroke();
      // Arrowheads pointing inward
      arrowHead(hax, sy,      0, +1);
      arrowHead(hax, sy + sh, 0, -1);
      // Label (rotated 90°)
      dimLabel(`${sel.h} px`, hax, hmy, true);
    }

    ctx.restore();
  }

  function drawPixelGrid() {
    const cw = canvas.width, ch = canvas.height;
    const imgL = Math.max(0, Math.floor(-offsetX / zoom));
    const imgT = Math.max(0, Math.floor(-offsetY / zoom));
    const imgR = Math.min(info.width,  Math.ceil((cw - offsetX) / zoom));
    const imgB = Math.min(info.height, Math.ceil((ch - offsetY) / zoom));

    const alpha = Math.min(1, (zoom - 6) / 4) * 0.25;
    ctx.strokeStyle = `rgba(255,255,255,${alpha})`;
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let ix = imgL; ix <= imgR; ix++) {
      const sx = Math.round(offsetX + ix * zoom) + 0.5;
      if (sx < RULER_SIZE || sx > cw) continue;
      ctx.moveTo(sx, Math.max(RULER_SIZE, offsetY + imgT * zoom));
      ctx.lineTo(sx, Math.min(ch, offsetY + imgB * zoom));
    }
    for (let iy = imgT; iy <= imgB; iy++) {
      const sy = Math.round(offsetY + iy * zoom) + 0.5;
      if (sy < RULER_SIZE || sy > ch) continue;
      ctx.moveTo(Math.max(RULER_SIZE, offsetX + imgL * zoom), sy);
      ctx.lineTo(Math.min(cw, offsetX + imgR * zoom), sy);
    }
    ctx.stroke();
  }

  function drawRuler() {
    const cw = canvas.width, ch = canvas.height;
    const rawStep   = 70 / zoom;
    const mag       = Math.pow(10, Math.floor(Math.log10(rawStep)));
    const norm      = rawStep / mag;
    const majorStep = Math.max(1, Math.round(norm < 2 ? mag : norm < 5 ? 2 * mag : 5 * mag));

    ctx.fillStyle = "rgba(16,16,18,0.94)";
    ctx.fillRect(RULER_SIZE, 0, cw - RULER_SIZE, RULER_SIZE);
    ctx.fillRect(0, RULER_SIZE, RULER_SIZE, ch - RULER_SIZE);
    ctx.fillStyle = "#0e0e10";
    ctx.fillRect(0, 0, RULER_SIZE, RULER_SIZE);

    ctx.strokeStyle = "#28283a"; ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(RULER_SIZE, RULER_SIZE); ctx.lineTo(cw, RULER_SIZE);
    ctx.moveTo(RULER_SIZE, RULER_SIZE); ctx.lineTo(RULER_SIZE, ch);
    ctx.stroke();

    ctx.strokeStyle = "#38384a"; ctx.fillStyle = "#505068"; ctx.font = "9px monospace";

    const x0 = Math.floor(-offsetX / zoom / majorStep) * majorStep;
    const x1 = Math.ceil((cw - offsetX) / zoom / majorStep) * majorStep;
    ctx.textAlign = "left"; ctx.textBaseline = "top";
    for (let ix = x0; ix <= x1; ix += majorStep) {
      const sx = Math.round(offsetX + ix * zoom) + 0.5;
      if (sx < RULER_SIZE || sx > cw) continue;
      ctx.beginPath(); ctx.moveTo(sx, RULER_SIZE - 6); ctx.lineTo(sx, RULER_SIZE); ctx.stroke();
      ctx.fillText(String(ix), sx + 2, 2);
    }

    const y0 = Math.floor(-offsetY / zoom / majorStep) * majorStep;
    const y1 = Math.ceil((ch - offsetY) / zoom / majorStep) * majorStep;
    for (let iy = y0; iy <= y1; iy += majorStep) {
      const sy = Math.round(offsetY + iy * zoom) + 0.5;
      if (sy < RULER_SIZE || sy > ch) continue;
      ctx.beginPath(); ctx.moveTo(RULER_SIZE - 6, sy); ctx.lineTo(RULER_SIZE, sy); ctx.stroke();
      ctx.save();
      ctx.translate(RULER_SIZE - 8, sy - 2); ctx.rotate(-Math.PI / 2);
      ctx.textAlign = "left"; ctx.textBaseline = "top";
      ctx.fillText(String(iy), 0, 0);
      ctx.restore();
    }
  }

  // ── Minimap ──────────────────────────────────────────────────────────────

  function drawMinimap() {
    if (!thumbImg) return;
    const cw = canvas.width, ch = canvas.height;
    const MMAP_MAX_W = 160, MMAP_MAX_H = 100, PAD = 5, MARGIN = 10;
    const aspect = info.width / info.height;
    const mw = aspect >= MMAP_MAX_W / MMAP_MAX_H ? MMAP_MAX_W : Math.max(8, Math.round(MMAP_MAX_H * aspect));
    const mh = aspect >= MMAP_MAX_W / MMAP_MAX_H ? Math.max(4, Math.round(MMAP_MAX_W / aspect)) : MMAP_MAX_H;

    const bx = cw - mw - PAD * 2 - MARGIN;
    const by = ch - mh - PAD * 2 - MARGIN - 44;
    mmapBx = bx; mmapBy = by; mmapMw = mw; mmapMh = mh;

    ctx.save();
    ctx.beginPath(); mmapRoundRect(bx - PAD, by - PAD, mw + PAD * 2, mh + PAD * 2, 5);
    ctx.fillStyle = "rgba(10,10,16,0.88)"; ctx.fill();
    ctx.strokeStyle = "#2a2a3a"; ctx.lineWidth = 1; ctx.stroke();
    ctx.restore();

    ctx.globalAlpha = 0.35;
    ctx.drawImage(thumbImg, bx, by, mw, mh);
    ctx.globalAlpha = 1;

    const vpL = Math.max(0, -offsetX / zoom);
    const vpT = Math.max(0, -offsetY / zoom);
    const vpR = Math.min(info.width,  (cw - offsetX) / zoom);
    const vpB = Math.min(info.height, (ch - offsetY) / zoom);
    const vx = bx + (vpL / info.width)  * mw;
    const vy = by + (vpT / info.height) * mh;
    const vw = Math.max(1, (vpR - vpL) / info.width  * mw);
    const vh = Math.max(1, (vpB - vpT) / info.height * mh);

    ctx.save();
    ctx.beginPath(); ctx.rect(bx, by, mw, mh); ctx.clip();
    ctx.beginPath(); ctx.rect(vx, vy, vw, vh); ctx.clip();
    ctx.drawImage(thumbImg, bx, by, mw, mh);
    ctx.restore();

    ctx.strokeStyle = "rgba(80,160,255,0.95)"; ctx.lineWidth = 1.5;
    ctx.strokeRect(vx, vy, vw, vh);
  }

  function mmapRoundRect(x: number, y: number, w: number, h: number, r: number) {
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.lineTo(x + w - r, y);     ctx.arcTo(x + w, y,     x + w, y + r,     r);
    ctx.lineTo(x + w, y + h - r); ctx.arcTo(x + w, y + h, x + w - r, y + h, r);
    ctx.lineTo(x + r, y + h);     ctx.arcTo(x,     y + h, x,     y + h - r, r);
    ctx.lineTo(x,     y + r);     ctx.arcTo(x,     y,     x + r, y,         r);
    ctx.closePath();
  }

  // ── Tile fetching ─────────────────────────────────────────────────────────

  function fetchVisibleTiles() {
    if (!container) return;
    fetchTilesFor(info, tileCache, loadingTiles, (p) => info?.path === p);
    if (pairInfo) fetchTilesFor(pairInfo, pairTileCache, pairLoadingTiles, (p) => pairInfo?.path === p);
  }

  function fetchTilesFor(
    imgInfo: ImageInfo,
    tiles: Map<string, HTMLImageElement>,
    loading: Set<string>,
    stillRelevant: (path: string) => boolean
  ) {
    const cw = container.offsetWidth, ch = container.offsetHeight;
    if (!cw || !ch) return;
    const tileScale  = Math.min(zoom, 1);
    const imgL = Math.max(0, Math.floor(-offsetX / zoom));
    const imgT = Math.max(0, Math.floor(-offsetY / zoom));
    const imgR = Math.min(imgInfo.width,  Math.ceil((cw - offsetX) / zoom));
    const imgB = Math.min(imgInfo.height, Math.ceil((ch - offsetY) / zoom));
    const currentPath = imgInfo.path;

    for (let ty = Math.floor(imgT / TILE_SIZE); ty < Math.ceil(imgB / TILE_SIZE); ty++) {
      for (let tx = Math.floor(imgL / TILE_SIZE); tx < Math.ceil(imgR / TILE_SIZE); tx++) {
        const sx = tx * TILE_SIZE, sy = ty * TILE_SIZE;
        const sw = Math.min(TILE_SIZE, imgInfo.width  - sx);
        const sh = Math.min(TILE_SIZE, imgInfo.height - sy);
        if (sw <= 0 || sh <= 0) continue;
        const key = `${sx},${sy},${sw},${sh},${tileScale.toFixed(3)}`;
        if (tiles.has(key) || loading.has(key)) continue;
        loading.add(key);
        invoke<string>("get_tile", { path: currentPath, x: sx, y: sy, w: sw, h: sh, scale: tileScale })
          .then(async (b64) => {
            try {
              const bmp = await createImageBitmap(await b64ToBlob(b64));
              if (stillRelevant(currentPath)) { tiles.set(key, bmp); scheduleDraw(); }
            } finally { loading.delete(key); }
          })
          .catch(() => loading.delete(key));
      }
    }
  }

  // ── Input handlers ────────────────────────────────────────────────────────

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      const rect = canvas.getBoundingClientRect();
      setZoom(zoom * (e.deltaY > 0 ? 0.85 : 1 / 0.85), e.clientX - rect.left, e.clientY - rect.top);
    } else {
      offsetX -= e.deltaX; offsetY -= e.deltaY; scheduleDraw();
    }
  }

  function inMinimap(cx: number, cy: number) {
    return mmapMw > 0 && cx >= mmapBx && cx <= mmapBx + mmapMw && cy >= mmapBy && cy <= mmapBy + mmapMh;
  }

  function panToMinimap(cx: number, cy: number) {
    if (!info || !container) return;
    const imgX = ((cx - mmapBx) / mmapMw) * info.width;
    const imgY = ((cy - mmapBy) / mmapMh) * info.height;
    const cw = container.offsetWidth, ch = container.offsetHeight;
    offsetX = cw / 2 - imgX * zoom;
    offsetY = ch / 2 - imgY * zoom;
    scheduleDraw(); fetchVisibleTiles();
  }

  function onMousedown(e: MouseEvent) {
    if (e.button !== 0) return;
    const rect = canvas.getBoundingClientRect();
    const cx = e.clientX - rect.left, cy = e.clientY - rect.top;

    // × delete buttons on selections take priority
    const hit = selHits.find(h => Math.hypot(cx - h.cx, cy - h.cy) < 10);
    if (hit) { selections = selections.filter(s => s.id !== hit.id); draw(); return; }

    if (inMinimap(cx, cy)) { minimapDragging = true; panToMinimap(cx, cy); return; }

    if (pairInfo && viewMode === "both") {
      const ch = container.offsetHeight;
      const sy = Math.round(ch * splitRatio);
      if (Math.abs(cy - sy) < 10) { splitDragging = true; return; }
    }

    if (selectMode) {
      const p = canvasToImg(cx, cy);
      selDraftStart = p; selDraftEnd = p;
      return;
    }

    dragging = true;
    dragStart = { x: e.clientX, y: e.clientY, ox: offsetX, oy: offsetY };
  }

  function onMousemove(e: MouseEvent) {
    const rect = canvas.getBoundingClientRect();
    const cx = e.clientX - rect.left, cy = e.clientY - rect.top;
    if (minimapDragging) { panToMinimap(cx, cy); return; }
    if (splitDragging) {
      const ch = container.offsetHeight;
      splitRatio = Math.max(0.1, Math.min(0.9, cy / ch));
      scheduleDraw(); return;
    }
    if (selDraftStart) { selDraftEnd = canvasToImg(cx, cy); scheduleDraw(); return; }
    if (!dragging) return;
    offsetX = dragStart.ox + (e.clientX - dragStart.x);
    offsetY = dragStart.oy + (e.clientY - dragStart.y);
    scheduleDraw();
  }

  function onMouseup() {
    if (selDraftStart && selDraftEnd) {
      const x = Math.min(selDraftStart.x, selDraftEnd.x);
      const y = Math.min(selDraftStart.y, selDraftEnd.y);
      const w = Math.abs(selDraftEnd.x - selDraftStart.x);
      const h = Math.abs(selDraftEnd.y - selDraftStart.y);
      const ix = Math.max(0, Math.round(x)), iy = Math.max(0, Math.round(y));
      const iw = Math.max(1, Math.round(w)), ih = Math.max(1, Math.round(h));
      if (iw >= 2 && ih >= 2) {
        const id = nextSelId++;
        selections = [...selections, { id, x: ix, y: iy, w: iw, h: ih, avgGV: null }];
        const imgPath = viewMode === "dark" && pairInfo ? pairInfo.path : info.path;
        invoke<number>("get_region_stats", { path: imgPath, x: ix, y: iy, w: iw, h: ih })
          .then(avg => { selections = selections.map(s => s.id === id ? { ...s, avgGV: avg } : s); draw(); })
          .catch(() => {});
      }
      selDraftStart = null; selDraftEnd = null;
      draw();
    }
    dragging = false; splitDragging = false; minimapDragging = false;
  }

  function b64ToBlob(b64: string): Promise<Blob> {
    return new Promise(resolve => {
      const bin = atob(b64);
      const buf = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) buf[i] = bin.charCodeAt(i);
      resolve(new Blob([buf], { type: "image/jpeg" }));
    });
  }
</script>

<div
  class="viewer"
  class:select-mode={selectMode}
  role="img"
  aria-label="Image viewer"
  bind:this={container}
  onwheel={onWheel}
  onmousedown={onMousedown}
  onmousemove={onMousemove}
  onmouseup={onMouseup}
  onmouseleave={onMouseup}
>
  <canvas bind:this={canvas}></canvas>
  <div class="zoom-badge">{Math.round(zoom * 100)}%</div>
  {#if pairInfo}
    <div class="view-toggle">
      <button class:active={viewMode === "light"} onclick={() => { viewMode = "light"; draw(); }}>Bright</button>
      <button class:active={viewMode === "both"}  onclick={() => { viewMode = "both";  draw(); }}>Both</button>
      <button class:active={viewMode === "dark"}  onclick={() => { viewMode = "dark";  draw(); }}>Dark</button>
    </div>
  {/if}
</div>

<style>
  .viewer { width: 100%; height: 100%; position: relative; cursor: grab; overflow: hidden; }
  .viewer:active { cursor: grabbing; }
  canvas { display: block; }
  .zoom-badge {
    position: absolute; bottom: 12px; right: 12px;
    background: rgba(10,10,16,0.78); border: 1px solid rgba(255,255,255,0.06);
    backdrop-filter: blur(10px); -webkit-backdrop-filter: blur(10px);
    color: #707090; padding: 3px 10px; border-radius: 20px;
    font-size: 11px; font-weight: 600; letter-spacing: 0.2px;
    pointer-events: none;
  }
  .view-toggle {
    position: absolute; bottom: 44px; right: 12px;
    display: flex; gap: 2px;
    background: rgba(10,10,16,0.78); border: 1px solid rgba(255,255,255,0.06);
    backdrop-filter: blur(10px); -webkit-backdrop-filter: blur(10px);
    border-radius: 7px; padding: 3px;
  }
  .view-toggle button {
    background: transparent; border: 1px solid transparent; color: #555570;
    border-radius: 5px; padding: 4px 10px; cursor: pointer; font-size: 11px; font-weight: 500;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
  }
  .view-toggle button:hover { background: rgba(255,255,255,0.05); color: #8888a8; }
  .view-toggle button.active { background: rgba(20,40,100,0.9); border-color: rgba(91,142,240,0.45); color: #88aaff; }
  .viewer.select-mode { cursor: crosshair; }
  .viewer.select-mode:active { cursor: crosshair; }
</style>
