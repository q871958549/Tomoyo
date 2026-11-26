<script lang="ts">
  import { Archive, CalendarDays, CheckSquare, Download, FileText, FolderOpen, Hash, Plus, Settings, Star } from '@lucide/svelte';
  import Button from '../../lib/ui/Button.svelte';
  import type { NoteFilter, TagCount } from './types';

  interface Props {
    activeFilter: NoteFilter;
    activeTag: string;
    total: number;
    pinnedCount: number;
    todoNotesCount: number;
    archivedCount: number;
    tags: TagCount[];
    onSelectFilter: (f: NoteFilter) => void;
    onSelectTag: (tag: string) => void;
    onCreateNote: () => void;
    onOpenDaily: () => void;
    onOpenDir: () => void;
    onExportAll: () => void;
    onOpenSettings: () => void;
  }

  let {
    activeFilter,
    activeTag,
    total,
    pinnedCount,
    todoNotesCount,
    archivedCount,
    tags,
    onSelectFilter,
    onSelectTag,
    onCreateNote,
    onOpenDaily,
    onOpenDir,
    onExportAll,
    onOpenSettings,
  }: Props = $props();

  let views = $derived([
    { id: 'all' as NoteFilter, label: '全部备忘', count: total, icon: FileText },
    { id: 'pinned' as NoteFilter, label: '置顶收藏', count: pinnedCount, icon: Star },
    { id: 'todo' as NoteFilter, label: '待办清单', count: todoNotesCount, icon: CheckSquare },
    { id: 'archived' as NoteFilter, label: '归档箱', count: archivedCount, icon: Archive },
  ]);
</script>

<aside class="ns-rail">
  <div class="ns-cta">
    <Button variant="primary" size="sm" onclick={onCreateNote} title="新建备忘录 (Ctrl+N)">
      <Plus size={14} />新建备忘
    </Button>
    <Button variant="secondary" size="sm" onclick={onOpenDaily} title="打开今日速记 (Daily Scratchpad)">
      <CalendarDays size={13} />今日
    </Button>
  </div>

  <div class="ns-grp">视图</div>
  {#each views as v}
    {@const Icon = v.icon}
    <button
      type="button"
      class="ns-btn {activeFilter === v.id && !activeTag ? 'is-active' : ''}"
      onclick={() => { onSelectFilter(v.id); onSelectTag(''); }}
    >
      <Icon size={14} />
      <span class="ns-lbl">{v.label}</span>
      <span class="ns-cnt">{v.count}</span>
    </button>
  {/each}

  <div class="ns-grp" style="margin-top:10px">标签</div>
  <div class="ns-tags">
    {#each tags as t}
      <button
        type="button"
        class="ns-btn {activeTag.toLowerCase() === t.tag.toLowerCase() ? 'is-active' : ''}"
        onclick={() => onSelectTag(activeTag.toLowerCase() === t.tag.toLowerCase() ? '' : t.tag)}
      >
        <Hash size={12.5} />
        <span class="ns-lbl">{t.tag.replace(/^#/, '')}</span>
        <span class="ns-cnt">{t.count}</span>
      </button>
    {:else}
      <div class="ns-empty-tags">正文输入 #标签 自动归类</div>
    {/each}
  </div>

  <div class="ns-foot">
    <button type="button" class="ns-btn" onclick={onOpenDir} title="在系统文件管理器打开本地目录">
      <FolderOpen size={13.5} /><span class="ns-lbl">存储目录</span>
    </button>
    <button type="button" class="ns-btn" onclick={onExportAll} title="导出全部活跃备忘录为 JSON 备份到剪贴板">
      <Download size={13.5} /><span class="ns-lbl">导出备份</span>
    </button>
    <button type="button" class="ns-btn" onclick={onOpenSettings} title="备忘录插件设置">
      <Settings size={13.5} /><span class="ns-lbl">插件设置</span>
    </button>
  </div>
</aside>

<style>
  .ns-rail {
    width: 186px;
    flex-shrink: 0;
    background: var(--sidebar);
    border-right: 1px solid var(--border);
    padding: 10px 8px;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .ns-cta {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px;
    margin-bottom: 10px;
  }
  .ns-grp {
    font-size: 11px;
    font-weight: 600;
    color: var(--muted);
    padding: 4px 8px;
  }
  .ns-tags {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ns-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 9px;
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
    flex-shrink: 0;
    transition: background-color 0.1s ease, color 0.1s ease;
  }
  .ns-btn:hover:not(.is-active) {
    background: var(--hover);
    color: var(--text);
  }
  .ns-btn.is-active {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 500;
  }
  .ns-lbl {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ns-cnt {
    font-size: 11px;
    opacity: 0.72;
    font-variant-numeric: tabular-nums;
  }
  .ns-empty-tags {
    padding: 6px 8px;
    font-size: 11.5px;
    color: var(--muted);
  }
  .ns-foot {
    padding-top: 8px;
    margin-top: 6px;
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
</style>
