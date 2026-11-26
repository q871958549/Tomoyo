<script lang="ts">
  import { onMount } from 'svelte';
  import { CalendarDays, Pin, Plus, Settings, Star, X } from '@lucide/svelte';
  import { applyThemeClass, installErrorTelemetry, postHost } from '../../lib/ipc';
  import type { ContextMenuItem } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import ContextMenu from '../../lib/ui/ContextMenu.svelte';
  import Input from '../../lib/ui/Input.svelte';
  import NotesEditor from './NotesEditor.svelte';
  import NotesSidebar from './NotesSidebar.svelte';
  import type { EditorViewMode, NoteFilter, NoteItem, NoteMutateRes, NotesListRes, TagCount } from './types';

  let notes = $state<NoteItem[]>([]);
  let tags = $state<TagCount[]>([]);
  let notesDir = $state('');
  let total = $state(0);
  let pinnedCount = $state(0);
  let todoNotesCount = $state(0);
  let archivedCount = $state(0);
  let activeFilter = $state<NoteFilter>('all');
  let activeTag = $state('');
  let query = $state('');
  let selectedFile = $state('');
  let titleDraft = $state('');
  let bodyDraft = $state('');
  let tagsDraft = $state<string[]>([]);
  let dirty = $state(false);
  let saving = $state(false);
  let viewMode = $state<EditorViewMode>('split');
  let topmostActive = $state(false);
  let toast = $state('本地备忘录工作台就绪 · Ctrl+N 新建 · Ctrl+F 搜索 · Ctrl+S 保存');
  let searchEl: HTMLInputElement | undefined = $state();
  let ctx = $state<{ open: boolean; x: number; y: number; item: NoteItem | null }>({ open: false, x: 0, y: 0, item: null });

  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  let autoSaveTimer: ReturnType<typeof setTimeout> | null = null;

  let current = $derived(notes.find((n) => n.file_name === selectedFile) ?? null);

  function syncDraftFrom(item: NoteItem | null) {
    titleDraft = item?.title ?? '';
    bodyDraft = item?.body ?? '';
    tagsDraft = item ? [...item.tags] : [];
    dirty = false;
  }

  function selectNote(fileName: string) {
    if (dirty && current && !current.archived) {
      void flushSave();
    }
    selectedFile = fileName;
    syncDraftFrom(notes.find((n) => n.file_name === fileName) ?? null);
  }

  async function reloadList(focusFile?: string) {
    if (!window.tomoyo?.callPlugin) return;
    try {
      const res = await window.tomoyo.callPlugin<NotesListRes>('list', {
        query: query.trim(),
        filter: activeFilter,
        tag: activeTag,
        focus_file: focusFile ?? '',
      });
      notes = Array.isArray(res?.notes) ? res.notes : [];
      tags = Array.isArray(res?.tags) ? res.tags : [];
      notesDir = res?.notes_dir || notesDir;
      total = res?.total ?? notes.length;
      pinnedCount = res?.pinned_count ?? 0;
      todoNotesCount = res?.todo_notes_count ?? 0;
      archivedCount = res?.archived_count ?? 0;

      const wanted = focusFile || selectedFile;
      if (wanted && notes.some((n) => n.file_name === wanted)) {
        selectedFile = wanted;
      } else {
        selectedFile = notes[0]?.file_name || '';
      }
      if (!dirty) {
        syncDraftFrom(notes.find((n) => n.file_name === selectedFile) ?? null);
      }
    } catch (err) {
      toast = `✗ 加载备忘录失败：${String(err)}`;
    }
  }

  function onSearchInput() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => { void reloadList(); }, 120);
  }

  function scheduleSave() {
    dirty = true;
    if (autoSaveTimer) clearTimeout(autoSaveTimer);
    autoSaveTimer = setTimeout(() => { void flushSave(); }, 600);
  }

  async function flushSave() {
    if (autoSaveTimer) {
      clearTimeout(autoSaveTimer);
      autoSaveTimer = null;
    }
    if (!window.tomoyo?.callPlugin || !current || current.archived) return;
    saving = true;
    try {
      const res = await window.tomoyo.callPlugin<NoteMutateRes>('save', {
        file_name: current.file_name,
        title: titleDraft.trim() || '未命名备忘',
        body: bodyDraft,
        tags: tagsDraft,
        pinned: current.pinned,
      });
      dirty = false;
      if (res?.note) {
        notes = notes.map((n) => (n.file_name === res.note!.file_name ? res.note! : n));
      }
      toast = '✓ 已保存到本地 Markdown';
      void reloadList(current.file_name);
    } catch (err) {
      toast = `✗ 保存失败：${String(err)}`;
    } finally {
      saving = false;
    }
  }

  async function createNote(opts: { title?: string; body?: string; daily?: boolean } = {}) {
    if (!window.tomoyo?.callPlugin) return;
    if (dirty) await flushSave();
    try {
      const res = await window.tomoyo.callPlugin<NoteMutateRes>('create', opts);
      activeFilter = 'all';
      activeTag = '';
      query = '';
      if (res?.note) {
        selectedFile = res.note.file_name;
        await reloadList(res.note.file_name);
        syncDraftFrom(res.note);
        toast = opts.daily ? '✓ 已打开今日速记' : '✓ 已新建本地备忘录';
      }
    } catch (err) {
      toast = `✗ 新建失败：${String(err)}`;
    }
  }

  async function togglePin(fileName: string) {
    if (!window.tomoyo?.callPlugin) return;
    try {
      const res = await window.tomoyo.callPlugin<NoteMutateRes>('toggle_pin', { file_name: fileName });
      toast = res?.pinned ? '★ 已置顶收藏' : '已取消置顶';
      await reloadList(fileName);
    } catch (err) {
      toast = `✗ 置顶操作失败：${String(err)}`;
    }
  }

  async function toggleTodo(lineIdx: number) {
    if (!window.tomoyo?.callPlugin || !current || current.archived) return;
    if (dirty) await flushSave();
    try {
      const res = await window.tomoyo.callPlugin<NoteMutateRes>('toggle_todo', {
        file_name: current.file_name,
        line_idx: lineIdx,
      });
      if (res?.note) {
        syncDraftFrom(res.note);
      }
      await reloadList(current.file_name);
    } catch (err) {
      toast = `✗ 切换待办失败：${String(err)}`;
    }
  }

  async function archiveOrRestore(item: NoteItem) {
    if (!window.tomoyo?.callPlugin) return;
    const method = item.archived ? 'restore' : 'archive';
    try {
      await window.tomoyo.callPlugin(method, { file_name: item.file_name });
      dirty = false;
      toast = item.archived ? '✓ 已从归档恢复' : '✓ 已移入 .archive/ 归档箱';
      await reloadList();
    } catch (err) {
      toast = `✗ 操作失败：${String(err)}`;
    }
  }

  async function deleteNote(item: NoteItem) {
    if (!window.tomoyo?.callPlugin) return;
    try {
      await window.tomoyo.callPlugin('delete', { file_name: item.file_name, archived: item.archived });
      dirty = false;
      toast = '✓ 已永久删除本地备忘文件';
      await reloadList();
    } catch (err) {
      toast = `✗ 删除失败：${String(err)}`;
    }
  }

  async function openWikiLink(target: string) {
    const clean = target.trim();
    if (!clean) return;
    const allHit = notes.find(
      (n) => n.file_name.toLowerCase() === clean.toLowerCase() || n.title.toLowerCase() === clean.toLowerCase(),
    );
    if (allHit) {
      selectNote(allHit.file_name);
      return;
    }
    if (activeFilter !== 'all' || activeTag || query) {
      activeFilter = 'all';
      activeTag = '';
      query = '';
      await reloadList();
      const refreshedHit = notes.find(
        (n) => n.file_name.toLowerCase() === clean.toLowerCase() || n.title.toLowerCase() === clean.toLowerCase(),
      );
      if (refreshedHit) {
        selectNote(refreshedHit.file_name);
        return;
      }
    }
    await createNote({ title: clean, body: '' });
  }

  async function exportAll() {
    if (!window.tomoyo?.callPlugin) return;
    try {
      const res = await window.tomoyo.callPlugin<NoteMutateRes>('export_all', {});
      postHost({ cmd: 'copy_text', text: String(res?.json || '[]') });
      toast = '✓ 全部活跃备忘 JSON 备份已复制到剪贴板';
    } catch (err) {
      toast = `✗ 导出失败：${String(err)}`;
    }
  }

  function relTime(ts: number): string {
    if (!ts) return '-';
    const diff = Math.max(0, Math.floor(Date.now() / 1000) - ts);
    if (diff < 60) return '刚刚';
    if (diff < 3600) return `${Math.floor(diff / 60)}分前`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}小时前`;
    const d = new Date(ts * 1000);
    return `${d.getMonth() + 1}-${d.getDate()}`;
  }

  let ctxItems = $derived.by<ContextMenuItem[]>(() => {
    const it = ctx.item;
    if (!it) return [];
    return [
      { id: 'paste', label: '复制并贴回原窗口' },
      { id: 'copy', label: '仅复制 Markdown' },
      { id: 'pin', label: it.pinned ? '取消置顶' : '置顶备忘', checked: it.pinned, disabled: it.archived },
      { id: 'open', label: '用系统默认程序打开' },
      { id: 'archive', label: it.archived ? '从归档恢复' : '移入归档箱' },
      { id: 'sep', label: '', separator: true },
      { id: 'delete', label: '永久删除', danger: true },
    ];
  });

  function onCtxSelect(id: string) {
    const it = ctx.item;
    if (!it) return;
    if (id === 'paste') postHost({ cmd: 'copy_and_paste', text: it.body });
    else if (id === 'copy') { postHost({ cmd: 'copy_text', text: it.body }); toast = '✓ 已复制 Markdown'; }
    else if (id === 'pin') void togglePin(it.file_name);
    else if (id === 'open') postHost({ cmd: 'open_path', path: it.path });
    else if (id === 'archive') void archiveOrRestore(it);
    else if (id === 'delete') void deleteNote(it);
  }

  onMount(() => {
    installErrorTelemetry('notes_workbench');
    applyThemeClass(window.tomoyo?.theme || 'dark');
    void reloadList(window.tomoyo?.initialQuery || '');
    window.tomoyo?.onEnter?.((action) => {
      void reloadList(action?.payload || '');
    });

    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement | null)?.tagName;
      const inEditor = tag === 'TEXTAREA' || tag === 'INPUT';
      if (e.ctrlKey && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        void createNote({});
      } else if (e.ctrlKey && e.key.toLowerCase() === 's') {
        e.preventDefault();
        void flushSave();
      } else if (e.ctrlKey && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        searchEl?.focus();
        searchEl?.select();
      } else if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 't') {
        e.preventDefault();
        topmostActive = !topmostActive;
        postHost({ cmd: 'toggle_pin_topmost' });
      } else if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && !inEditor && notes.length > 0) {
        e.preventDefault();
        const curIdx = notes.findIndex((n) => n.file_name === selectedFile);
        const delta = e.key === 'ArrowDown' ? 1 : -1;
        const nextIdx = curIdx < 0 ? 0 : (curIdx + delta + notes.length) % notes.length;
        selectNote(notes[nextIdx].file_name);
      } else if (e.key === 'Escape' && !e.defaultPrevented) {
        postHost({ cmd: 'close' });
      }
    };
    window.addEventListener('keydown', onKey);
    return () => { window.removeEventListener('keydown', onKey); };
  });
</script>

<div class="wb-shell">
  <header class="wb-head">
    <div class="na-hl">
      <strong>本地备忘录工作台</strong>
      <span class="t-pill">{total} 条笔记</span>
      {#if todoNotesCount > 0}<span class="t-pill" style="color:var(--warn)">{todoNotesCount} 待办</span>{/if}
    </div>
    <div class="na-hr">
      <div style="width:240px">
        <Input type="search" size="sm" bind:inputEl={searchEl} bind:value={query} placeholder="搜索标题、正文、#标签… (Ctrl+F)" oninput={onSearchInput} />
      </div>
      <Button variant="primary" size="sm" onclick={() => void createNote({})}><Plus size={13.5} />新建</Button>
      <Button variant="secondary" size="sm" onclick={() => void createNote({ daily: true })}><CalendarDays size={13.5} />今日速记</Button>
      <Button variant="icon" size="sm" active={topmostActive} title="便签置顶 (Ctrl+Shift+T)" onclick={() => { topmostActive = !topmostActive; postHost({ cmd: 'toggle_pin_topmost' }); }}><Pin size={14} /></Button>
      <Button variant="icon" size="sm" title="备忘录插件设置" onclick={() => postHost({ cmd: 'open_settings', target: 'plugins:dev.tomoyo.notes' })}><Settings size={14} /></Button>
      <Button variant="icon" size="sm" title="关闭窗口" onclick={() => postHost({ cmd: 'close' })}><X size={14} /></Button>
    </div>
  </header>

  <div class="na-main">
    <NotesSidebar
      {activeFilter}
      {activeTag}
      {total}
      {pinnedCount}
      {todoNotesCount}
      {archivedCount}
      {tags}
      onSelectFilter={(f) => { activeFilter = f; void reloadList(); }}
      onSelectTag={(t) => { activeTag = t; void reloadList(); }}
      onCreateNote={() => void createNote({})}
      onOpenDaily={() => void createNote({ daily: true })}
      onOpenDir={() => notesDir && postHost({ cmd: 'open_path', path: notesDir })}
      onExportAll={() => void exportAll()}
      onOpenSettings={() => postHost({ cmd: 'open_settings', target: 'plugins:dev.tomoyo.notes' })}
    />

    <section class="na-list">
      <div class="na-scroll">
        {#each notes as item}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div
            class="na-card {item.file_name === selectedFile ? 'is-sel' : ''}"
            data-note-file={item.file_name}
            onclick={() => selectNote(item.file_name)}
            oncontextmenu={(e) => { e.preventDefault(); selectNote(item.file_name); ctx = { open: true, x: e.clientX, y: e.clientY, item }; }}
          >
            <div class="na-ctop">
              <span class="na-ctitle">
                {#if item.pinned}<Star size={12} class="na-pin" />{/if}
                {item.title}
              </span>
              <span class="na-cts">{relTime(item.modified_secs)}</span>
            </div>
            <div class="na-csnip">{item.snippet}</div>
            {#if item.tags.length > 0 || item.todo_open > 0 || item.todo_done > 0}
              <div class="na-cmeta">
                {#if item.todo_open > 0 || item.todo_done > 0}
                  <span class="t-pill">☑ {item.todo_done}/{item.todo_open + item.todo_done}</span>
                {/if}
                {#each item.tags.slice(0, 3) as tg}
                  <span class="t-pill">{tg}</span>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <div class="wb-empty">当前筛选下暂无备忘录</div>
        {/each}
      </div>
    </section>

    <NotesEditor
      note={current}
      {titleDraft}
      {bodyDraft}
      {tagsDraft}
      {dirty}
      {saving}
      {viewMode}
      {topmostActive}
      onChangeTitle={(v) => { titleDraft = v; scheduleSave(); }}
      onChangeBody={(v) => { bodyDraft = v; scheduleSave(); }}
      onChangeTags={(t) => { tagsDraft = t; scheduleSave(); }}
      onChangeView={(m) => (viewMode = m)}
      onSaveNow={() => void flushSave()}
      onTogglePin={() => current && void togglePin(current.file_name)}
      onToggleTodo={(lineIdx) => void toggleTodo(lineIdx)}
      onArchiveOrRestore={() => current && void archiveOrRestore(current)}
      onDelete={() => current && void deleteNote(current)}
      onOpenWikiLink={(target) => void openWikiLink(target)}
      onSelectTag={(t) => { activeTag = t; void reloadList(); }}
      onCreateFirst={() => void createNote({})}
      onOpenDaily={() => void createNote({ daily: true })}
      onToggleTopmost={() => { topmostActive = !topmostActive; postHost({ cmd: 'toggle_pin_topmost' }); }}
      onToast={(m) => (toast = m)}
    />
  </div>

  <footer class="wb-foot">
    <span class="wb-foot-toast">{toast}</span>
    <span>Ctrl+N 新建 · Ctrl+S 立即落盘 · Ctrl+Shift+T 便签置顶 · Esc 关闭</span>
  </footer>
</div>

<ContextMenu open={ctx.open} x={ctx.x} y={ctx.y} items={ctxItems} onselect={onCtxSelect} onclose={() => (ctx.open = false)} />

<style>
  .na-hl, .na-hr { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .na-main { flex: 1; display: flex; min-height: 0; overflow: hidden; }
  .na-list { width: 266px; flex-shrink: 0; border-right: 1px solid var(--border); background: var(--canvas); display: flex; flex-direction: column; min-height: 0; }
  .na-scroll { flex: 1; overflow-y: auto; padding: 8px; display: flex; flex-direction: column; gap: 6px; }
  .na-card { background: var(--surface); border: 1px solid var(--border); border-radius: 9px; padding: 9px 10px; cursor: pointer; transition: border-color 0.12s ease; }
  .na-card:hover { border-color: var(--border-strong); }
  .na-card.is-sel { border-color: var(--accent); box-shadow: 0 0 0 2px var(--ring); }
  .na-ctop { display: flex; align-items: center; justify-content: space-between; gap: 6px; margin-bottom: 4px; }
  .na-ctitle { display: inline-flex; align-items: center; gap: 4px; font-size: 13px; font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .na-ctitle :global(.na-pin) { color: var(--warn); flex-shrink: 0; }
  .na-cts { font-size: 11px; color: var(--muted); flex-shrink: 0; font-variant-numeric: tabular-nums; }
  .na-csnip { font-size: 12px; color: var(--text-secondary); line-height: 1.45; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; word-break: break-word; }
  .na-cmeta { margin-top: 6px; display: flex; gap: 4px; flex-wrap: wrap; }
</style>
