<script lang="ts">
  import { Copy, Star, Volume2 } from '@lucide/svelte';
  import Button from '../../lib/ui/Button.svelte';

  export interface EngineResult {
    engine_id?: string;
    engine_name: string;
    ok: boolean;
    translated?: string;
    detected_lang?: string;
    phonetic?: string;
    extra_dict?: string;
    error?: string;
    elapsed_ms?: number;
  }

  let {
    result,
    targetLang,
    saved,
    onCopy,
    onSaveWord,
    onSpeak,
  }: {
    result: EngineResult;
    targetLang: string;
    saved: boolean;
    onCopy: (text: string, label: string) => void;
    onSaveWord: (r: EngineResult) => void;
    onSpeak: (text: string, lang?: string) => void;
  } = $props();
</script>

<div class="t-card eng-card">
  <div class="eng-head">
    <div class="eng-meta">
      <span class="dot {result.ok ? 'is-ok' : 'is-err'}"></span>
      <span class="eng-name">{result.engine_name}</span>
      {#if result.detected_lang}<span class="t-pill">{result.detected_lang} → {targetLang}</span>{/if}
      <span class="t-pill">{result.elapsed_ms || 0} ms</span>
    </div>
    {#if result.ok && result.translated}
      <div class="eng-acts">
        <Button variant="icon" size="sm" title="复制译文" onclick={() => onCopy(result.translated || '', `${result.engine_name} 译文`)}>
          <Copy size={13} />
        </Button>
        <Button variant="icon" size="sm" active={saved} title={saved ? '已收藏' : '加入生词本'} onclick={() => onSaveWord(result)}>
          <Star size={13} />
        </Button>
        <Button variant="icon" size="sm" title="朗读译文" onclick={() => onSpeak(result.translated || '', targetLang)}>
          <Volume2 size={13} />
        </Button>
      </div>
    {/if}
  </div>

  {#if result.ok && result.translated}
    <div class="eng-text">{result.translated}</div>
    {#if result.phonetic || result.extra_dict}
      <div class="dict-box">
        {#if result.phonetic}<div class="phonetic">/{result.phonetic}/</div>{/if}
        {#if result.extra_dict}
          {#each result.extra_dict.split(/\r?\n/).filter(Boolean) as line}
            <div>{line}</div>
          {/each}
        {/if}
      </div>
    {/if}
  {:else}
    <div class="eng-err">{result.error || '翻译失败'}</div>
  {/if}
</div>

<style>
  .eng-card { padding: 12px 14px; }
  .eng-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-bottom: 8px; }
  .eng-meta { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .eng-acts { display: flex; align-items: center; gap: 6px; }
  .dot { width: 7px; height: 7px; border-radius: 999px; flex-shrink: 0; }
  .dot.is-ok { background: var(--ok); }
  .dot.is-err { background: var(--danger); }
  .eng-name { font-weight: 600; font-size: 13px; }
  .eng-text { white-space: pre-wrap; word-break: break-word; font-size: 14px; line-height: 1.7; user-select: text; }
  .eng-err { font-size: 12.5px; color: var(--danger); }
  .dict-box {
    margin-top: 9px; padding: 8px 11px; border-radius: 8px; background: var(--canvas);
    border: 1px solid var(--border); font-size: 12px; line-height: 1.6; color: var(--text-secondary);
  }
  .phonetic { color: var(--accent); font-weight: 500; font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; }
</style>
