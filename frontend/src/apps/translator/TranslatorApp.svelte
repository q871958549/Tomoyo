<script lang="ts">
  import { onMount } from 'svelte';
  import {
    ArrowLeftRight,
    BookMarked,
    Copy,
    History,
    Pin,
    Settings,
    Sparkles,
    Star,
    Trash2,
    Volume2,
    X,
  } from '@lucide/svelte';
  import { applyThemeClass, installErrorTelemetry, postHost, subscribeHost } from '../../lib/ipc';
  import type { ContextMenuItem, SelectOption } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import ContextMenu from '../../lib/ui/ContextMenu.svelte';
  import Select from '../../lib/ui/Select.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import TranslatorDrawer, { type HistItem, type WordItem } from './TranslatorDrawer.svelte';

  interface EngineResult {
    engine_name: string;
    ok: boolean;
    translated?: string;
    detected_lang?: string;
    phonetic?: string;
    extra_dict?: string;
    elapsed_ms?: number;
    error?: string;
  }
  interface TranslateBatch {
    source_text: string;
    target_lang: string;
    trace_id?: string;
    results: EngineResult[];
  }

  const boot = (window.__TOMOYO_TR_INIT__ ?? {}) as {
    theme?: string; text?: string; target_lang?: string;
    engines?: string[]; history?: HistItem[]; wordbook?: WordItem[];
  };

  const srcLangs: SelectOption[] = [
    { value: 'auto', label: '自动检测' },
    { value: 'zh-Hans', label: '中文' },
    { value: 'en', label: 'English' },
    { value: 'ja', label: '日本語' },
    { value: 'ko', label: '한국어' },
    { value: 'fr', label: 'Français' },
    { value: 'de', label: 'Deutsch' },
  ];
  const tgtLangs: SelectOption[] = [
    { value: 'auto', label: '中 ⇄ 英 自动' },
    { value: 'zh-Hans', label: '简体中文' },
    { value: 'en', label: 'English' },
    { value: 'ja', label: '日本語' },
    { value: 'ko', label: '한국어' },
    { value: 'fr', label: 'Français' },
    { value: 'de', label: 'Deutsch' },
  ];

  let srcLang = $state('auto');
  let tgtLang = $state(boot.target_lang || 'auto');
  let sourceText = $state(boot.text || '');
  let autoTranslate = $state(true);
  let loading = $state(false);
  let batch = $state<TranslateBatch | null>(null);
  let history = $state<HistItem[]>(Array.isArray(boot.history) ? boot.history : []);
  let wordbook = $state<WordItem[]>(Array.isArray(boot.wordbook) ? boot.wordbook : []);
  let drawer = $state<'wordbook' | 'history' | null>(null);
  let pinnedTop = $state(false);
  let clearArmed = $state(false);
  let toast = $state('已就绪 · 多源并行翻译');
  let ctxMenu = $state({ open: false, x: 0, y: 0 });
  let srcEl: HTMLTextAreaElement | undefined = $state();
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  function isSaved(word: string): boolean {
    const low = word.trim().toLowerCase();
    return !!low && wordbook.some((w) => w.word.toLowerCase() === low);
  }

  function runTranslate() {
    const clean = sourceText.trim();
    if (!clean) { toast = '请先输入或粘贴待翻译文本'; return; }
    loading = true;
    toast = '正在并行请求翻译引擎…';
    postHost({ cmd: 'run_translate', text: clean, source_lang: srcLang, target_lang: tgtLang });
  }

  function onSourceInput() {
    if (!autoTranslate) return;
    if (debounceTimer) clearTimeout(debounceTimer);
    if (!sourceText.trim()) { batch = null; return; }
    debounceTimer = setTimeout(runTranslate, 480);
  }

  function swapLangs() {
    if (tgtLang === 'auto' || tgtLang === 'zh-Hans') tgtLang = 'en';
    else if (tgtLang === 'en') tgtLang = 'zh-Hans';
    else if (srcLang !== 'auto') {
      const tmp = srcLang; srcLang = tgtLang; tgtLang = tmp;
    }
    if (sourceText.trim()) runTranslate();
  }

  function copyTxt(text: string, label: string) {
    if (!text) return;
    postHost({ cmd: 'copy_text', text });
    toast = `✓ 已复制${label}（${text.length} 字）`;
  }

  function speakTxt(text: string, langHint = '') {
    const clean = text.trim();
    if (!clean || !('speechSynthesis' in window)) return;
    try {
      window.speechSynthesis.cancel();
      const u = new SpeechSynthesisUtterance(clean.slice(0, 600));
      if (langHint === 'zh-Hans' || /[\u4e00-\u9fff]/.test(clean)) u.lang = 'zh-CN';
      else if (langHint === 'ja') u.lang = 'ja-JP';
      else if (langHint === 'ko') u.lang = 'ko-KR';
      else if (langHint === 'fr') u.lang = 'fr-FR';
      else if (langHint === 'de') u.lang = 'de-DE';
      else u.lang = 'en-US';
      window.speechSynthesis.speak(u);
      toast = '正在朗读…';
    } catch { toast = '朗读启动失败'; }
  }

  function saveWord(r: EngineResult) {
    const src = (batch?.source_text || sourceText).trim();
    if (!src || !r.translated) return;
    postHost({
      cmd: 'tr_save_word',
      word: src,
      translation: r.translated,
      phonetic: r.phonetic || '',
      note: r.engine_name,
    });
  }

  function togglePin() {
    pinnedTop = !pinnedTop;
    postHost({ cmd: 'toggle_pin_topmost' });
  }

  function clearHistory() {
    if (!clearArmed) {
      clearArmed = true;
      setTimeout(() => { clearArmed = false; }, 3500);
      return;
    }
    clearArmed = false;
    postHost({ cmd: 'tr_clear_history' });
  }

  function loadEntry(text: string, target?: string) {
    sourceText = text;
    if (target) tgtLang = target;
    if (text.trim()) runTranslate();
  }

  let srcMenuItems = $derived<ContextMenuItem[]>([
    { id: 'translate', label: '立即翻译', shortcut: 'Ctrl+Enter', disabled: !sourceText.trim() },
    { id: 'copy', label: '复制原文', shortcut: 'Ctrl+C', disabled: !sourceText.trim() },
    { id: 'speak', label: '朗读原文', disabled: !sourceText.trim() },
    { id: 'sep1', label: '', separator: true },
    { id: 'clear', label: '清空输入区', danger: true, disabled: !sourceText },
  ]);

  function onSrcMenuSelect(id: string) {
    if (id === 'translate') runTranslate();
    else if (id === 'copy') copyTxt(sourceText, '原文');
    else if (id === 'speak') speakTxt(sourceText);
    else if (id === 'clear') { sourceText = ''; batch = null; srcEl?.focus(); }
  }

  onMount(() => {
    installErrorTelemetry('translator');
    applyThemeClass(boot.theme || 'dark');
    if (sourceText.trim()) runTranslate();
    else srcEl?.focus();

    const unsub = subscribeHost((msg) => {
      const ev = String(msg.event || '');
      if (ev === 'translate_result') {
        loading = false;
        const st = msg.store as { history?: HistItem[]; wordbook?: WordItem[] } | undefined;
        if (Array.isArray(st?.history)) history = st.history;
        if (Array.isArray(st?.wordbook)) wordbook = st.wordbook;
        batch = (msg.data as TranslateBatch) || null;
        const okCount = (batch?.results || []).filter((r) => r.ok).length;
        toast = `✓ 翻译完成（${okCount}/${batch?.results?.length || 0} 引擎成功）`;
      } else if (ev === 'translate_reload_req') {
        if (typeof msg.target_lang === 'string' && msg.target_lang) tgtLang = msg.target_lang;
        if (typeof msg.text === 'string' && msg.text.trim()) loadEntry(msg.text, tgtLang);
        else srcEl?.focus();
      } else if (ev === 'tr_store_updated') {
        if (Array.isArray(msg.history)) history = msg.history as HistItem[];
        if (Array.isArray(msg.wordbook)) wordbook = msg.wordbook as WordItem[];
        if (msg.action === 'save_word') toast = '★ 已收藏到生词本';
        else if (msg.action === 'delete_word') toast = '已从生词本移除';
        else if (msg.action === 'clear_history') toast = '已清空翻译历史';
      }
    });

    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === 'Enter') { e.preventDefault(); runTranslate(); }
      else if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 't') { e.preventDefault(); togglePin(); }
      else if (e.key === 'Escape' && !e.defaultPrevented) {
        if (drawer) drawer = null;
        else postHost({ cmd: 'close' });
      }
    };
    window.addEventListener('keydown', onKey);
    return () => { unsub(); window.removeEventListener('keydown', onKey); };
  });
</script>

<div class="wb-shell">
  <header class="wb-head">
    <div class="lang-capsule">
      <Select value={srcLang} options={srcLangs} size="sm" minWidth="104px" onchange={(v) => { srcLang = v; if (sourceText.trim()) runTranslate(); }} />
      <Button variant="icon" size="sm" title="互换语言" onclick={swapLangs}><ArrowLeftRight size={14} /></Button>
      <Select value={tgtLang} options={tgtLangs} size="sm" minWidth="122px" onchange={(v) => { tgtLang = v; if (sourceText.trim()) runTranslate(); }} />
      <Button variant="primary" size="sm" disabled={loading} onclick={runTranslate}>
        <Sparkles size={13} />
        {loading ? '翻译中…' : '翻译'}
      </Button>
    </div>

    <div class="hdr-tools">
      <div class="auto-pill" title="输入停止 480ms 后自动翻译">
        <span>自动</span>
        <Switch bind:checked={autoTranslate} />
      </div>
      <Button variant="ghost" size="sm" active={drawer === 'wordbook'} onclick={() => (drawer = drawer === 'wordbook' ? null : 'wordbook')}>
        <BookMarked size={14} />生词本
      </Button>
      <Button variant="ghost" size="sm" active={drawer === 'history'} onclick={() => (drawer = drawer === 'history' ? null : 'history')}>
        <History size={14} />历史
      </Button>
      <Button variant="icon" size="sm" title="翻译插件设置" onclick={() => postHost({ cmd: 'open_settings', target: 'plugins:dev.tomoyo.translate' })}>
        <Settings size={14} />
      </Button>
      <Button variant="icon" size="sm" active={pinnedTop} title="窗口置顶 (Ctrl+Shift+T)" onclick={togglePin}>
        <Pin size={14} />
      </Button>
      <Button variant="icon" size="sm" title="关闭 (Esc)" onclick={() => postHost({ cmd: 'close' })}>
        <X size={14} />
      </Button>
    </div>
  </header>

  <div class="tr-body">
    <section class="tr-main">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="t-card src-card"
        oncontextmenu={(e) => { e.preventDefault(); ctxMenu = { open: true, x: e.clientX, y: e.clientY }; }}
      >
        <textarea
          bind:this={srcEl}
          bind:value={sourceText}
          spellcheck="false"
          placeholder="输入或粘贴待翻译文本，按 Ctrl+Enter 立即多源并行翻译…"
          oninput={onSourceInput}
        ></textarea>
        <div class="src-foot">
          <span class="t-pill">{sourceText.length} 字符</span>
          <div class="src-acts">
            <Button variant="icon" size="sm" title="朗读原文" disabled={!sourceText.trim()} onclick={() => speakTxt(sourceText)}>
              <Volume2 size={14} />
            </Button>
            <Button variant="icon" size="sm" title="复制原文" disabled={!sourceText.trim()} onclick={() => copyTxt(sourceText, '原文')}>
              <Copy size={14} />
            </Button>
            <Button variant="icon" size="sm" title="清空" disabled={!sourceText} onclick={() => { sourceText = ''; batch = null; srcEl?.focus(); }}>
              <Trash2 size={14} />
            </Button>
          </div>
        </div>
      </div>

      <div class="res-stack">
        {#if loading}
          {#each [1, 2] as _}
            <div class="t-card eng-card">
              <div class="t-skeleton" style="width:120px;height:14px;margin-bottom:10px"></div>
              <div class="t-skeleton" style="width:88%;height:15px;margin-bottom:7px"></div>
              <div class="t-skeleton" style="width:62%;height:15px"></div>
            </div>
          {/each}
        {:else if batch}
          {#each batch.results as r}
            {@const saved = isSaved(batch.source_text)}
            <div class="t-card eng-card">
              <div class="eng-head">
                <div class="eng-meta">
                  <span class="dot {r.ok ? 'is-ok' : 'is-err'}"></span>
                  <span class="eng-name">{r.engine_name}</span>
                  {#if r.detected_lang}<span class="t-pill">{r.detected_lang} → {batch.target_lang}</span>{/if}
                  <span class="t-pill">{r.elapsed_ms || 0} ms</span>
                </div>
                {#if r.ok && r.translated}
                  <div class="src-acts">
                    <Button variant="icon" size="sm" title="复制译文" onclick={() => copyTxt(r.translated || '', `${r.engine_name} 译文`)}>
                      <Copy size={13} />
                    </Button>
                    <Button variant="icon" size="sm" active={saved} title={saved ? '已收藏' : '加入生词本'} onclick={() => saveWord(r)}>
                      <Star size={13} />
                    </Button>
                    <Button variant="icon" size="sm" title="朗读译文" onclick={() => speakTxt(r.translated || '', batch?.target_lang)}>
                      <Volume2 size={13} />
                    </Button>
                  </div>
                {/if}
              </div>

              {#if r.ok && r.translated}
                <div class="eng-text">{r.translated}</div>
                {#if r.phonetic || r.extra_dict}
                  <div class="dict-box">
                    {#if r.phonetic}<div class="phonetic">/{r.phonetic}/</div>{/if}
                    {#if r.extra_dict}
                      {#each r.extra_dict.split(/\r?\n/).filter(Boolean) as line}
                        <div>{line}</div>
                      {/each}
                    {/if}
                  </div>
                {/if}
              {:else}
                <div class="eng-err">{r.error || '翻译失败'}</div>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
    </section>

    {#if drawer}
      <TranslatorDrawer
        mode={drawer}
        {wordbook}
        {history}
        {clearArmed}
        onClearHistory={clearHistory}
        onLoadEntry={loadEntry}
        onClose={() => (drawer = null)}
      />
    {/if}
  </div>

  <footer class="wb-foot">
    <span class="wb-foot-toast">{toast}</span>
    <span>Ctrl+Enter 翻译 · Ctrl+Shift+T 置顶 · Esc 退出释放内存</span>
  </footer>
</div>

<ContextMenu
  open={ctxMenu.open}
  x={ctxMenu.x}
  y={ctxMenu.y}
  items={srcMenuItems}
  onselect={onSrcMenuSelect}
  onclose={() => (ctxMenu.open = false)}
/>

<style>
  .lang-capsule, .hdr-tools, .src-acts { display: flex; align-items: center; gap: 6px; }
  .auto-pill { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--muted); margin-right: 4px; }
  .tr-body { flex: 1; display: flex; min-height: 0; overflow: hidden; }
  .tr-main { flex: 1; overflow-y: auto; padding: 14px 16px; display: flex; flex-direction: column; gap: 12px; min-width: 0; }
  .src-card { padding: 12px 14px 10px; }
  .src-card textarea {
    width: 100%; min-height: 92px; max-height: 200px; border: 0; outline: none; resize: vertical;
    background: transparent; color: var(--text); font-family: inherit; font-size: 14px; line-height: 1.65;
  }
  .src-foot {
    display: flex; align-items: center; justify-content: space-between;
    margin-top: 6px; padding-top: 8px; border-top: 1px solid var(--border);
  }
  .res-stack { display: flex; flex-direction: column; gap: 10px; }
  .eng-card { padding: 12px 14px; }
  .eng-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-bottom: 8px; }
  .eng-meta { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
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
