<script lang="ts">
  import { onMount } from 'svelte';
  import {
    Copy,
    ExternalLink,
    FileCode2,
    Languages,
    ListX,
    Maximize2,
    Minimize2,
    Pin,
    Scissors,
    Settings,
    WrapText,
    X,
  } from '@lucide/svelte';
  import { applyThemeClass, installErrorTelemetry, postHost } from '../../lib/ipc';
  import Button from '../../lib/ui/Button.svelte';

  interface OcrBoot {
    theme?: string; text?: string; raw_text?: string; image_path?: string; image_url?: string;
    lang_tag?: string; lang_note?: string; src_dims?: [number, number]; proc_dims?: [number, number]; elapsed_ms?: number;
  }

  const boot = (window.__TOMOYO_OCR_DATA__ ?? {}) as OcrBoot;

  let text = $state(boot.text || '');
  let showingRaw = $state(false);
  let actualZoom = $state(false);
  let pinnedTop = $state(false);
  let imgLoaded = $state(false);
  let toast = $state('已加载最近一次 OCR 识别结果');
  let taEl: HTMLTextAreaElement | undefined = $state();

  let dimsLabel = $derived.by(() => {
    const s = boot.src_dims || [0, 0];
    const p = boot.proc_dims || s;
    return s[0] !== p[0] || s[1] !== p[1] ? `${s[0]}×${s[1]} → ${p[0]}×${p[1]}` : `${s[0]}×${s[1]}`;
  });
  let lineCount = $derived(text ? text.split(/\r?\n/).filter((l) => l.trim().length > 0).length : 0);
  let imgName = $derived((boot.image_path || '').split(/[\\/]/).pop() || '原图对照');

  function copyAll() {
    postHost({ cmd: 'copy_text', text });
    toast = `✓ 已复制全部 ${text.length} 字`;
  }

  function copySelection() {
    const sel = taEl ? text.substring(taEl.selectionStart, taEl.selectionEnd) : '';
    if (!sel) { toast = '请先在右侧文本框划选内容'; return; }
    postHost({ cmd: 'copy_text', text: sel });
    toast = `✓ 已复制选中（${sel.length} 字）`;
  }

  function mergeSingleLine() {
    text = text.split(/\r?\n/).map((s) => s.trim()).filter(Boolean).join(' ');
    toast = '已合并为单行';
  }

  function stripBlankLines() {
    text = text.split(/\r?\n/).map((s) => s.trimEnd()).filter((s) => s.trim().length > 0).join('\n');
    toast = '已清理空白行';
  }

  function toggleRaw() {
    showingRaw = !showingRaw;
    text = showingRaw ? (boot.raw_text || boot.text || '') : (boot.text || '');
  }

  function translateText() {
    const sel = taEl ? text.substring(taEl.selectionStart, taEl.selectionEnd).trim() : '';
    const target = sel || text.trim();
    if (!target) { toast = '当前没有可翻译的文本'; return; }
    postHost({ cmd: 'open_translator', text: target, target_lang: '' });
    toast = `已发送 ${target.length} 字到 Tomoyo 多源翻译`;
  }

  function togglePin() {
    pinnedTop = !pinnedTop;
    postHost({ cmd: 'toggle_pin_topmost' });
  }

  onMount(() => {
    installErrorTelemetry('ocr_viewer');
    applyThemeClass(boot.theme || 'dark');

    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'c') { e.preventDefault(); copyAll(); }
      else if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 't') { e.preventDefault(); togglePin(); }
      else if (e.key === 'Escape') postHost({ cmd: 'close' });
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
</script>

<div class="wb-shell">
  <header class="wb-head">
    <div class="ocr-meta">
      <strong>OCR 审校与编辑</strong>
      <span class="t-pill">{boot.lang_tag || 'WinRT'}</span>
      <span class="t-pill">{dimsLabel}</span>
      <span class="t-pill">{boot.elapsed_ms || 0} ms</span>
      <span class="t-pill">{lineCount} 行 · {text.length} 字</span>
    </div>
    <div class="ocr-acts">
      <Button variant="primary" size="sm" onclick={copyAll}><Copy size={13} />复制全文</Button>
      <Button variant="secondary" size="sm" onclick={copySelection}><Scissors size={13} />复制选中</Button>
      <Button variant="secondary" size="sm" onclick={translateText}><Languages size={13} />一键翻译</Button>
      <Button variant="ghost" size="sm" onclick={mergeSingleLine}><WrapText size={13} />合并单行</Button>
      <Button variant="ghost" size="sm" onclick={stripBlankLines}><ListX size={13} />去空行</Button>
      <Button variant="ghost" size="sm" active={showingRaw} onclick={toggleRaw}><FileCode2 size={13} />{showingRaw ? '清洗文本' : '原始输出'}</Button>
      <Button variant="icon" size="sm" title="打开原图" onclick={() => boot.image_path && postHost({ cmd: 'open_path', path: boot.image_path })}><ExternalLink size={14} /></Button>
      <Button variant="icon" size="sm" title="OCR 插件设置" onclick={() => postHost({ cmd: 'open_settings', target: 'plugins:dev.tomoyo.ocr' })}><Settings size={14} /></Button>
      <Button variant="icon" size="sm" active={pinnedTop} title="置顶" onclick={togglePin}><Pin size={14} /></Button>
      <Button variant="icon" size="sm" title="关闭" onclick={() => postHost({ cmd: 'close' })}><X size={14} /></Button>
    </div>
  </header>

  <div class="ocr-work">
    <section class="pane-img">
      <div class="pane-bar">
        <span class="img-name">{imgName}</span>
        <Button variant="ghost" size="sm" onclick={() => (actualZoom = !actualZoom)}>
          {#if actualZoom}<Minimize2 size={13} />适应窗口{:else}<Maximize2 size={13} />100% 原图{/if}
        </Button>
      </div>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="stage {actualZoom ? 'is-actual' : ''}" onclick={() => (actualZoom = !actualZoom)}>
        {#if boot.image_url}
          <img
            src={boot.image_url}
            alt="OCR Source"
            style:display={imgLoaded ? 'block' : 'none'}
            onload={() => (imgLoaded = true)}
          />
        {/if}
        {#if !imgLoaded}
          <span class="t-desc">原图未加载</span>
        {/if}
      </div>
    </section>

    <section class="pane-txt">
      <div class="pane-bar">
        <span>识别文本（可直接修改、划选、复制）</span>
        <span class="t-pill">{showingRaw ? '原始 WinRT' : '智能清洗'}</span>
      </div>
      {#if boot.lang_note}
        <div class="warn-bar">⚠ {boot.lang_note}</div>
      {/if}
      <textarea bind:this={taEl} bind:value={text} spellcheck="false"></textarea>
    </section>
  </div>

  <footer class="wb-foot">
    <span class="wb-foot-toast">{toast}</span>
    <span>Ctrl+Shift+C 复制全文 · Ctrl+Shift+T 置顶 · Esc 关闭释放内存</span>
  </footer>
</div>

<style>
  .ocr-meta, .ocr-acts { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .ocr-work { flex: 1; display: flex; min-height: 0; overflow: hidden; }
  .pane-img { width: 46%; display: flex; flex-direction: column; border-right: 1px solid var(--border); background: var(--img-stage); min-width: 0; }
  .pane-txt { flex: 1; display: flex; flex-direction: column; background: var(--surface); min-width: 0; }
  .pane-bar {
    height: 36px; padding: 0 12px; display: flex; align-items: center; justify-content: space-between;
    background: var(--sidebar); border-bottom: 1px solid var(--border); font-size: 12px; color: var(--muted);
  }
  .img-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .stage { flex: 1; overflow: auto; display: flex; align-items: center; justify-content: center; padding: 14px; cursor: zoom-in; }
  .stage.is-actual { align-items: flex-start; justify-content: flex-start; cursor: zoom-out; }
  .stage img { max-width: 100%; max-height: 100%; object-fit: contain; border-radius: 6px; box-shadow: var(--shadow-pop); }
  .stage.is-actual img { max-width: none; max-height: none; }
  .warn-bar {
    margin: 10px 12px 0; padding: 8px 11px; border-radius: 8px;
    background: var(--warn-soft); border: 1px solid var(--warn); color: var(--warn); font-size: 12px;
  }
  .pane-txt textarea {
    flex: 1; width: 100%; border: 0; outline: none; resize: none; padding: 14px 16px;
    background: transparent; color: var(--text); font-family: "Alibaba PuHuiTi 3.0", "Cascadia Mono", Consolas, monospace;
    font-size: 13.5px; line-height: 1.7;
  }
</style>
