<script module lang="ts">
  export interface WordItem {
    word: string;
    translation: string;
    phonetic?: string;
    note?: string;
  }
  export interface HistItem {
    source: string;
    translated: string;
    engine: string;
    target_lang: string;
  }
</script>

<script lang="ts">
  import { X } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import Button from '../../lib/ui/Button.svelte';
  import Input from '../../lib/ui/Input.svelte';

  interface Props {
    mode: 'wordbook' | 'history';
    wordbook: WordItem[];
    history: HistItem[];
    clearArmed: boolean;
    onClearHistory: () => void;
    onLoadEntry: (text: string, targetLang: string) => void;
    onClose: () => void;
  }

  let { mode, wordbook, history, clearArmed, onClearHistory, onLoadEntry, onClose }: Props = $props();
  let query = $state('');

  let filteredWords = $derived(
    wordbook.filter((w) => {
      const q = query.trim().toLowerCase();
      return !q || w.word.toLowerCase().includes(q) || w.translation.toLowerCase().includes(q);
    })
  );
  let filteredHist = $derived(
    history.filter((h) => {
      const q = query.trim().toLowerCase();
      return !q || h.source.toLowerCase().includes(q) || h.translated.toLowerCase().includes(q);
    })
  );
</script>

<aside class="drawer">
  <div class="drawer-head">
    <strong>{mode === 'wordbook' ? `生词本 (${wordbook.length})` : `历史 (${history.length})`}</strong>
    <div class="drawer-acts">
      {#if mode === 'history' && history.length > 0}
        <Button variant="danger" size="sm" onclick={onClearHistory}>{clearArmed ? '确认清空' : '清空'}</Button>
      {/if}
      <Button variant="icon" size="sm" onclick={onClose}><X size={14} /></Button>
    </div>
  </div>
  <div class="drawer-q">
    <Input type="search" size="sm" bind:value={query} placeholder="筛选词条…" />
  </div>
  <div class="drawer-list">
    {#if mode === 'wordbook'}
      {#each filteredWords as w}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="d-card" onclick={() => onLoadEntry(w.word, 'auto')}>
          <div class="d-src">{w.word} {#if w.phonetic}<span class="phonetic">{w.phonetic}</span>{/if}</div>
          <div class="d-tr">{w.translation}</div>
          <div class="d-foot">
            <span>{w.note || '生词'}</span>
            <Button variant="danger" size="sm" onclick={(e) => { e.stopPropagation(); postHost({ cmd: 'tr_delete_word', word: w.word }); }}>移除</Button>
          </div>
        </div>
      {:else}
        <div class="empty-tip">暂无收藏生词</div>
      {/each}
    {:else}
      {#each filteredHist as h}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="d-card" onclick={() => onLoadEntry(h.source, h.target_lang || 'auto')}>
          <div class="d-src">{h.source}</div>
          <div class="d-tr">{h.translated}</div>
          <div class="d-foot"><span>{h.engine} · → {h.target_lang}</span></div>
        </div>
      {:else}
        <div class="empty-tip">暂无翻译历史</div>
      {/each}
    {/if}
  </div>
</aside>

<style>
  .drawer {
    width: 276px; flex-shrink: 0; border-left: 1px solid var(--border); background: var(--sidebar);
    display: flex; flex-direction: column; min-height: 0;
  }
  .drawer-head { padding: 10px 12px; border-bottom: 1px solid var(--border); display: flex; align-items: center; justify-content: space-between; font-size: 13px; }
  .drawer-acts { display: flex; align-items: center; gap: 6px; }
  .drawer-q { padding: 8px 10px; border-bottom: 1px solid var(--border); }
  .drawer-list { flex: 1; overflow-y: auto; padding: 8px; display: flex; flex-direction: column; gap: 6px; }
  .d-card {
    background: var(--surface); border: 1px solid var(--border); border-radius: 8px;
    padding: 8px 10px; cursor: pointer; transition: border-color 0.12s ease;
  }
  .d-card:hover { border-color: var(--accent); }
  .d-src { font-weight: 500; font-size: 12.5px; word-break: break-word; }
  .d-tr { font-size: 12px; color: var(--muted); margin-top: 3px; word-break: break-word; }
  .d-foot { display: flex; align-items: center; justify-content: space-between; margin-top: 5px; font-size: 11px; color: var(--muted); }
  .phonetic { color: var(--accent); font-weight: 500; font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; }
  .empty-tip { text-align: center; padding: 28px 10px; font-size: 12px; color: var(--muted); }
</style>
