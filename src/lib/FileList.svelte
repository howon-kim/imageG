<script lang="ts">
  type SidebarItem = { path: string; origIdx: number; hasPair: boolean };
  let { items, currentIndex, onselect }: {
    items: SidebarItem[];
    currentIndex: number;
    onselect: (origIdx: number) => void;
  } = $props();

  function filename(path: string) {
    return path.split("/").pop() ?? path;
  }
</script>

<div class="file-list">
  {#each items as item}
    <button
      class="file-item"
      class:active={item.origIdx === currentIndex}
      onclick={() => onselect(item.origIdx)}
      title={item.path}
    >
      {#if item.hasPair}
        <span class="pair-dot" title="Has paired image"></span>
      {/if}
      <span class="name">{filename(item.path)}</span>
    </button>
  {/each}
</div>

<style>
  .file-list { display: flex; flex-direction: column; padding: 4px 0; }
  .file-item {
    display: flex; align-items: center; gap: 6px;
    text-align: left; padding: 5px 10px 5px 12px;
    background: none; border: none; border-left: 2px solid transparent;
    color: #585870; cursor: pointer; font-size: 12px; width: 100%;
    white-space: nowrap; overflow: hidden;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
  }
  .file-item:hover { background: #15151c; color: #9898b0; border-left-color: #282838; }
  .file-item.active { background: #111128; color: #b8c0f0; border-left-color: #5b8ef0; }
  .name { overflow: hidden; text-overflow: ellipsis; }
  .pair-dot {
    flex-shrink: 0;
    width: 6px; height: 6px; border-radius: 50%;
    background: #50e898; opacity: 0.6;
  }
  .file-item.active .pair-dot { opacity: 1; }
</style>
