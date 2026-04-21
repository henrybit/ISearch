<script lang="ts">
  import '../app.css';
  let { children } = $props();
</script>

<svelte:head>
  <title>ISearch - 智能文件搜索</title>
  <meta name="description" content="快速、准确、易用的本地文件系统搜索引擎" />
</svelte:head>

{@render children()}
