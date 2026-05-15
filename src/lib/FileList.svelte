<script lang="ts">
  let { files, currentIndex, onselect }: {
    files: string[];
    currentIndex: number;
    onselect: (i: number) => void;
  } = $props();

  function filename(path: string) {
    return path.split("/").pop() ?? path;
  }
</script>

<div class="file-list">
  {#each files as f, i}
    <button
      class="file-item"
      class:active={i === currentIndex}
      onclick={() => onselect(i)}
      title={f}
    >
      {filename(f)}
    </button>
  {/each}
</div>

<style>
  .file-list { display: flex; flex-direction: column; padding: 4px 0; }
  .file-item {
    text-align: left; padding: 5px 10px; background: none; border: none;
    color: #bbb; cursor: pointer; font-size: 12px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    border-radius: 0;
  }
  .file-item:hover { background: #2a2a2a; }
  .file-item.active { background: #1e3a6a; color: #fff; }
</style>
