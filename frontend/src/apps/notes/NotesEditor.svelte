<script lang="ts">
  import {
    Archive, ArchiveRestore, CheckSquare, Clock, Code2, Columns2, Copy, Edit3, ExternalLink,
    Eye, Heading2, Link2, Pin, Plus, RotateCcw, Send, Star, Trash2, X,
  } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import Button from '../../lib/ui/Button.svelte';
  import { renderNoteMarkdown } from './markdown';
  import type { EditorViewMode, NoteItem } from './types';

  interface Props {
    note: NoteItem | null;
    titleDraft: string;
    bodyDraft: string;
    tagsDraft: string[];
    dirty: boolean;
    saving: boolean;
    viewMode: EditorViewMode;
    topmostActive: boolean;
    onChangeTitle: (v: string) => void;
    onChangeBody: (v: string) => void;
    onChangeTags: (tags: string[]) => void;
    onChangeView: (v: EditorViewMode) => void;
    onSaveNow: () => void;
    onTogglePin: () => void;
    onToggleTodo: (lineIdx: number) => void;
    onArchiveOrRestore: () => void;
    onDelete: () => void;
    onOpenWikiLink: (target: string) => void;
    onSelectTag: (tag: string) => void;
    onCreateFirst: () => void;
    onOpenDaily: () => void;
    onToggleTopmost: () => void;
    onToast: (msg: string) => void;
  }

  let {
    note,
    titleDraft,
    bodyDraft,
    tagsDraft,
    dirty,
    saving,
    viewMode,
    topmostActive,
    onChangeTitle,
    onChangeBody,
    onChangeTags,
    onChangeView,
    onSaveNow,
    onTogglePin,
    onToggleTodo,
    onArchiveOrRestore,
    onDelete,
    onOpenWikiLink,
    onSelectTag,
    onCreateFirst,
    onOpenDaily,
    onToggleTopmost,
    onToast,
  }: Props = $props();

  let newTagInput = $state('');
  let textareaEl: HTMLTextAreaElement | undefined = $state();

  let renderedHtml = $derived(renderNoteMarkdown(bodyDraft));
  let wordCount = $derived(bodyDraft.replace(/\s+/g, '').length);
  let lineCount = $derived(bodyDraft ? bodyDraft.split('\n').length : 0);

  function addTagFromInput() {
    const raw = newTagInput.trim().replace(/^#+/, '');
    newTagInput = '';
    if (!raw) return;
    const norm = `#${raw}`;
    if (!tagsDraft.some((t) => t.toLowerCase() === norm.toLowerCase())) {
      onChangeTags([...tagsDraft, norm]);
    }
  }

  function removeTag(tag: string) {
    onChangeTags(tagsDraft.filter((t) => t !== tag));
  }

  function insertSnippet(snippet: string) {
    const el = textareaEl;
    if (!el) {
      onChangeBody(bodyDraft ? `${bodyDraft}\n${snippet}` : snippet);
      return;
    }
    const start = el.selectionStart ?? bodyDraft.length;
    const end = el.selectionEnd ?? bodyDraft.length;
    const next = `${bodyDraft.slice(0, start)}${snippet}${bodyDraft.slice(end)}`;
    onChangeBody(next);
    requestAnimationFrame(() => {
      el.focus();
      const pos = start + snippet.length;
      el.setSelectionRange(pos, pos);
    });
  }

  function handleEditorKey(e: KeyboardEvent) {
    if (e.key === 'Tab' && textareaEl) {
      e.preventDefault();
      insertSnippet('  ');
    }
  }

  function handlePreviewClick(e: MouseEvent) {
    const target = e.target as HTMLElement | null;
    if (!target) return;
    const todoEl = target.closest('[data-todo-line]') as HTMLElement | null;
    if (todoEl) {
      const idx = Number(todoEl.dataset.todoLine);
      if (Number.isFinite(idx)) {
        e.preventDefault();
        onToggleTodo(idx);
      }
      return;
    }
    const wikiEl = target.closest('[data-wikilink]') as HTMLElement | null;
    if (wikiEl?.dataset.wikilink) {
      e.preventDefault();
      onOpenWikiLink(wikiEl.dataset.wikilink);
      return;
    }
    const tagEl = target.closest('[data-tag]') as HTMLElement | null;
    if (tagEl?.dataset.tag) {
      e.preventDefault();
      onSelectTag(tagEl.dataset.tag);
      return;
    }
    const linkEl = target.closest('[data-ext-url]') as HTMLElement | null;
    if (linkEl?.dataset.extUrl) {
      e.preventDefault();
      postHost({ cmd: 'open_external', url: linkEl.dataset.extUrl });
    }
  }

  function fmtDate(ts: number): string {
    if (!ts) return '-';
    const d = new Date(ts * 1000);
    const p = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
  }
</script>

<section class="ne-pane">
  {#if !note}
    <div class="ne-empty">
      <div class="ne-empty-t">开放式纯本地 `.md` 备忘录工作台</div>
      <div class="ne-empty-d">数据透明保存在 `.md` + YAML Frontmatter，支持双机 `sync_dir` 同步、待办清单与 `[[双链]]`。</div>
      <div class="ne-empty-btns">
        <Button variant="primary" size="md" onclick={onCreateFirst}><Plus size={14} />新建第一条备忘</Button>
        <Button variant="secondary" size="md" onclick={onOpenDaily}>打开今日速记</Button>
      </div>
    </div>
  {:else}
    <div class="ne-top">
      <input
        type="text"
        class="ne-title"
        value={titleDraft}
        readonly={note.archived}
        placeholder="输入备忘录标题…"
        oninput={(e) => onChangeTitle((e.currentTarget as HTMLInputElement).value)}
      />
      <span class="ne-status {dirty ? 'is-dirty' : ''}">
        {note.archived ? '已归档只读' : saving ? '保存中…' : dirty ? '待同步' : '已落盘'}
      </span>
      <div class="ne-seg" role="group" aria-label="视图模式">
        <button type="button" class={viewMode === 'edit' ? 'is-on' : ''} onclick={() => onChangeView('edit')} title="纯编辑"><Edit3 size={13} /></button>
        <button type="button" class={viewMode === 'split' ? 'is-on' : ''} onclick={() => onChangeView('split')} title="双栏对照"><Columns2 size={13} /></button>
        <button type="button" class={viewMode === 'preview' ? 'is-on' : ''} onclick={() => onChangeView('preview')} title="纯预览"><Eye size={13} /></button>
      </div>
      <Button variant="ghost" size="sm" active={note.pinned} disabled={note.archived} onclick={onTogglePin} title="置顶备忘卡片">
        <Star size={13.5} />
      </Button>
      <Button variant="ghost" size="sm" active={topmostActive} onclick={onToggleTopmost} title="切换窗口置顶便签模式 (Ctrl+Shift+T)">
        <Pin size={13.5} />
      </Button>
      <Button variant="primary" size="sm" onclick={() => { postHost({ cmd: 'copy_and_paste', text: bodyDraft }); onToast('✓ 已贴回原窗口'); }} title="复制正文并贴回呼出前的窗口">
        <Send size={13} />贴回
      </Button>
      <Button variant="secondary" size="sm" onclick={() => { postHost({ cmd: 'copy_text', text: bodyDraft }); onToast('✓ 已复制 Markdown'); }} title="复制 Markdown 内容">
        <Copy size={13} />复制
      </Button>
      <Button variant="icon" size="sm" onclick={() => postHost({ cmd: 'open_path', path: note.path })} title="使用系统关联程序 / Quiet.md 打开原文件">
        <ExternalLink size={13.5} />
      </Button>
      <Button variant="icon" size="sm" onclick={onArchiveOrRestore} title={note.archived ? '从归档恢复' : '移入归档箱'}>
        {#if note.archived}<ArchiveRestore size={13.5} />{:else}<Archive size={13.5} />{/if}
      </Button>
      <Button variant="danger" size="sm" onclick={onDelete} title="永久删除文件">
        <Trash2 size={13.5} />
      </Button>
    </div>

    <div class="ne-bar">
      {#if !note.archived}
        <div class="ne-tools">
          <button type="button" class="ne-tool" onclick={() => insertSnippet('\n- [ ] ')} title="插入待办清单"><CheckSquare size={12.5} />待办</button>
          <button type="button" class="ne-tool" onclick={() => insertSnippet('\n## ')} title="插入二级标题"><Heading2 size={12.5} />小节</button>
          <button type="button" class="ne-tool" onclick={() => insertSnippet('\n```\n\n```\n')} title="插入代码块"><Code2 size={12.5} />代码</button>
          <button type="button" class="ne-tool" onclick={() => insertSnippet('[[]]')} title="插入双向链接"><Link2 size={12.5} />双链</button>
          <button type="button" class="ne-tool" onclick={() => insertSnippet(`[${fmtDate(Math.floor(Date.now() / 1000))}] `)} title="插入当前时间"><Clock size={12.5} />时间</button>
          <button type="button" class="ne-tool" onclick={onSaveNow} title="立即保存 (Ctrl+S)"><RotateCcw size={12} />立即落盘</button>
        </div>
      {/if}
      <div class="ne-tags-row">
        {#each tagsDraft as tg}
          <span class="ne-chip">
            <button type="button" onclick={() => onSelectTag(tg)}>{tg}</button>
            {#if !note.archived}
              <button type="button" class="ne-chip-x" onclick={() => removeTag(tg)} aria-label="移除标签"><X size={11} /></button>
            {/if}
          </span>
        {/each}
        {#if !note.archived}
          <input
            type="text"
            class="ne-tag-in"
            bind:value={newTagInput}
            placeholder="+ 标签回车"
            maxlength={24}
            onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addTagFromInput(); } }}
          />
        {/if}
      </div>
    </div>

    <div class="ne-body ne-mode-{viewMode}">
      {#if viewMode !== 'preview'}
        <textarea
          bind:this={textareaEl}
          class="ne-ta"
          value={bodyDraft}
          readonly={note.archived}
          placeholder="开始记录……支持 Markdown、- [ ] 待办、#标签、[[双向链接]]"
          spellcheck="false"
          oninput={(e) => onChangeBody((e.currentTarget as HTMLTextAreaElement).value)}
          onkeydown={handleEditorKey}
        ></textarea>
      {/if}
      {#if viewMode !== 'edit'}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="ne-prev" onclick={handlePreviewClick}>{@html renderedHtml}</div>
      {/if}
    </div>

    {#if note.backlinks.length > 0}
      <div class="ne-backlinks">
        <span class="ne-bl-lbl">反向链接 ({note.backlinks.length}):</span>
        {#each note.backlinks as bl}
          <button type="button" class="ne-bl-chip" onclick={() => onOpenWikiLink(bl.file_name)}>{bl.title}</button>
        {/each}
      </div>
    {/if}

    <div class="ne-meta-foot">
      <span>{note.file_name}{note.source ? ` · 来源: ${note.source}` : ''}</span>
      <span>{wordCount} 字 · {lineCount} 行 · 待办 {note.todo_done}/{note.todo_open + note.todo_done} · 更新 {fmtDate(note.modified_secs)}</span>
    </div>
  {/if}
</section>

<style>
  .ne-pane { flex: 1; min-width: 0; display: flex; flex-direction: column; background: var(--surface); }
  .ne-empty { margin: auto; max-width: 420px; text-align: center; padding: 24px; display: flex; flex-direction: column; gap: 10px; align-items: center; }
  .ne-empty-t { font-size: 16px; font-weight: 600; color: var(--text); }
  .ne-empty-d { font-size: 12.5px; color: var(--muted); line-height: 1.6; }
  .ne-empty-btns { display: flex; gap: 8px; margin-top: 6px; }
  .ne-top { display: flex; align-items: center; gap: 6px; padding: 8px 12px; border-bottom: 1px solid var(--border); flex-wrap: wrap; }
  .ne-title { flex: 1; min-width: 140px; border: 0; outline: none; background: transparent; font-size: 15px; font-weight: 600; color: var(--text); }
  .ne-status { font-size: 11.5px; color: var(--ok); white-space: nowrap; }
  .ne-status.is-dirty { color: var(--warn); }
  .ne-seg { display: inline-flex; border: 1px solid var(--border); border-radius: 6px; overflow: hidden; background: var(--canvas); }
  .ne-seg button { width: 26px; height: 26px; display: grid; place-items: center; color: var(--muted); cursor: pointer; }
  .ne-seg button.is-on { background: var(--accent-soft); color: var(--accent); }
  .ne-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 6px 12px; border-bottom: 1px solid var(--border); background: var(--canvas); flex-wrap: wrap; }
  .ne-tools { display: flex; align-items: center; gap: 4px; flex-wrap: wrap; }
  .ne-tool { display: inline-flex; align-items: center; gap: 4px; height: 24px; padding: 0 7px; border-radius: 5px; font-size: 11.5px; color: var(--text-secondary); cursor: pointer; }
  .ne-tool:hover { background: var(--hover); color: var(--text); }
  .ne-tags-row { display: flex; align-items: center; gap: 5px; flex-wrap: wrap; }
  .ne-chip { display: inline-flex; align-items: center; gap: 3px; height: 22px; padding: 0 7px; border-radius: 99px; background: var(--accent-soft); color: var(--accent); font-size: 11.5px; }
  .ne-chip button { cursor: pointer; }
  .ne-chip-x { opacity: 0.7; display: inline-flex; }
  .ne-chip-x:hover { opacity: 1; }
  .ne-tag-in { width: 78px; height: 22px; padding: 0 6px; border: 1px dashed var(--border); border-radius: 99px; background: transparent; font-size: 11.5px; outline: none; }
  .ne-tag-in:focus { border-color: var(--accent); border-style: solid; width: 96px; }
  .ne-body { flex: 1; min-height: 0; display: grid; }
  .ne-mode-edit { grid-template-columns: 1fr; }
  .ne-mode-preview { grid-template-columns: 1fr; }
  .ne-mode-split { grid-template-columns: 1fr 1fr; }
  .ne-ta { width: 100%; height: 100%; border: 0; outline: none; resize: none; padding: 14px 16px; background: transparent; color: var(--text); font-family: var(--font-mono); font-size: 13px; line-height: 1.7; border-right: 1px solid var(--border); overflow-y: auto; }
  .ne-mode-edit .ne-ta { border-right: 0; }
  .ne-prev { padding: 14px 18px; overflow-y: auto; font-size: 13.5px; line-height: 1.65; color: var(--text); word-break: break-word; }
  .ne-prev :global(.md-h1) { font-size: 18px; font-weight: 700; margin: 4px 0 8px; }
  .ne-prev :global(.md-h2) { font-size: 15px; font-weight: 600; margin: 12px 0 6px; }
  .ne-prev :global(.md-h3) { font-size: 13.5px; font-weight: 600; margin: 8px 0 4px; }
  .ne-prev :global(.md-p) { margin: 2px 0; }
  .ne-prev :global(.md-gap) { height: 7px; }
  .ne-prev :global(.md-hr) { border: 0; border-top: 1px solid var(--border); margin: 10px 0; }
  .ne-prev :global(.md-quote) { border-left: 3px solid var(--accent); padding: 2px 10px; color: var(--text-secondary); margin: 4px 0; }
  .ne-prev :global(.md-pre) { background: var(--canvas); border: 1px solid var(--border); border-radius: 7px; padding: 8px 10px; font-family: var(--font-mono); font-size: 12px; overflow-x: auto; margin: 6px 0; }
  .ne-prev :global(.md-code) { background: var(--subtle); padding: 1px 5px; border-radius: 4px; font-family: var(--font-mono); font-size: 12px; }
  .ne-prev :global(.md-todo) { display: flex; align-items: flex-start; gap: 7px; margin: 3px 0; cursor: pointer; }
  .ne-prev :global(.md-todo input) { margin-top: 4px; accent-color: var(--accent); cursor: pointer; }
  .ne-prev :global(.md-todo.is-done span) { text-decoration: line-through; color: var(--muted); }
  .ne-prev :global(.md-li) { display: flex; gap: 7px; margin: 2px 0; }
  .ne-prev :global(.md-dot) { color: var(--accent); }
  .ne-prev :global(.md-link) { color: var(--accent); text-decoration: underline; }
  .ne-prev :global(.md-tag) { color: var(--accent); background: var(--accent-soft); padding: 0 5px; border-radius: 4px; font-size: 12px; cursor: pointer; }
  .ne-prev :global(.md-wikilink) { color: var(--ok); background: var(--ok-soft); padding: 0 6px; border-radius: 4px; font-weight: 500; cursor: pointer; }
  .ne-backlinks { display: flex; align-items: center; gap: 6px; padding: 6px 14px; border-top: 1px solid var(--border); background: var(--canvas); flex-wrap: wrap; }
  .ne-bl-lbl { font-size: 11.5px; color: var(--muted); font-weight: 500; }
  .ne-bl-chip { padding: 2px 8px; border-radius: 5px; background: var(--surface); border: 1px solid var(--border); font-size: 11.5px; color: var(--text); cursor: pointer; }
  .ne-bl-chip:hover { border-color: var(--accent); color: var(--accent); }
  .ne-meta-foot { display: flex; justify-content: space-between; gap: 8px; padding: 4px 14px; border-top: 1px solid var(--border); font-size: 11px; color: var(--muted); background: var(--sidebar); flex-wrap: wrap; }
</style>
