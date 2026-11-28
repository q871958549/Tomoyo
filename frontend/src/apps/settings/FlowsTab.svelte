<script lang="ts">
  import Icon from '../../lib/ui/Icon.svelte';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { McpProbeResult, McpServerEntry, SelectOption, SnippetEntry, WorkflowDef, WorkflowStep } from '../../lib/types';
  import McpServersCard from './McpServersCard.svelte';
  import Button from '../../lib/ui/Button.svelte';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import Input from '../../lib/ui/Input.svelte';
  import Select from '../../lib/ui/Select.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface DryRunState { flow_id?: string; trace?: Array<{ index: number; kind: string; output: string }>; effects?: string[]; }
  interface Props { lang: Lang; sub?: string; flows: WorkflowDef[]; flowsPath: string; snippets?: SnippetEntry[]; snippetsPath?: string; dryRes?: DryRunState | null; mcpServers?: McpServerEntry[]; mcpPath?: string; probingMcp?: string; mcpProbe?: McpProbeResult | null; onSaveFlows: (next: WorkflowDef[]) => void; onSaveSnippets?: (next: SnippetEntry[]) => void; onSaveMcp?: (next: McpServerEntry[]) => void; onProbeMcp?: (srv: McpServerEntry) => void; }

  let { lang, sub = 'flows', flows, flowsPath, snippets = [], snippetsPath = '', dryRes = null, mcpServers = [], mcpPath = '', probingMcp = '', mcpProbe = null, onSaveFlows, onSaveSnippets, onSaveMcp, onProbeMcp }: Props = $props();
  let isEn = $derived(lang === 'en');
  let hkEditId = $state<string | null>(null), dryId = $state<string | null>(null), drySample = $state('  Tomoyo\n   Shell  '), editingId = $state<string | null>(null);
  let sKw = $state(''), sTitle = $state(''), sTags = $state(''), sTpl = $state(''), sEdit = $state<string | null>(null);

  const FLOW_HK_PRESETS: SelectOption[] = [
    { value: 'Alt+Shift+V', label: 'Alt+Shift+V' },
    { value: 'Alt+Shift+T', label: 'Alt+Shift+T' },
    { value: 'Alt+Shift+N', label: 'Alt+Shift+N' },
    { value: 'Alt+Shift+A', label: 'Alt+Shift+A' },
    { value: 'F2', label: 'F2' },
    { value: 'F3', label: 'F3' },
  ];

  const STEP_META: Array<{ value: string; zh: string; en: string; needsVal?: boolean; ph?: string }> = [
    { value: 'read_clipboard', zh: '读取剪贴板', en: 'Read Clipboard' },
    { value: 'capture_region', zh: '区域截图', en: 'Capture Region' },
    { value: 'capture_ocr', zh: '截图并 OCR', en: 'Capture & OCR' },
    { value: 'capture_translate', zh: '截图并翻译', en: 'Capture & Translate' },
    { value: 'trim', zh: '首尾去空白', en: 'Trim Whitespace' },
    { value: 'single_line', zh: '合并为单行', en: 'Merge Single Line' },
    { value: 'trim_lines', zh: '去除多余空行', en: 'Strip Blank Lines' },
    { value: 'uppercase', zh: '转为大写', en: 'Uppercase' },
    { value: 'lowercase', zh: '转为小写', en: 'Lowercase' },
    { value: 'url_encode', zh: 'URL 编码', en: 'URL Encode' },
    { value: 'template', zh: '模板拼装', en: 'Template', needsVal: true, ph: '例如 [{{date}}] {{text}}' },
    { value: 'llm_transform', zh: '本地/AI 文本处理', en: 'Local / AI Transform', needsVal: true, ph: '系统提示词（调用本地 Ollama 或 DPAPI OpenAI 接口）' },
    { value: 'copy_text', zh: '复制到剪贴板', en: 'Copy to Clipboard' },
    { value: 'paste_text', zh: '直贴回原窗口', en: 'Paste to Window' },
    { value: 'launch_translate', zh: '多源翻译', en: 'Launch Translate' },
    { value: 'save_note', zh: '存为本地备忘', en: 'Save Local Note', needsVal: true, ph: '备忘标题（可留空）' },
    { value: 'open_url', zh: '打开网页', en: 'Open URL', needsVal: true, ph: 'https://example.com/?q={{text}}' },
    { value: 'fill_query', zh: '预填启动器指令', en: 'Fill Launcher Query', needsVal: true, ph: '例如 note + 摘录 :: {{text}}' },
    { value: 'toast', zh: '底部提示', en: 'Show Toast', needsVal: true, ph: '完成提示语' },
  ];

  let stepOptions = $derived<SelectOption[]>(
    STEP_META.map((s) => ({ value: s.value, label: isEn ? s.en : s.zh }))
  );

  let draftName = $state('');
  let draftKeyword = $state('');
  let draftDesc = $state('');
  let draftSteps = $state<WorkflowStep[]>([
    { kind: 'read_clipboard', value: '' },
    { kind: 'single_line', value: '' },
    { kind: 'copy_text', value: '' },
  ]);

  function stepLabel(kind: string): string {
    const m = STEP_META.find((s) => s.value === kind);
    return m ? (isEn ? m.en : m.zh) : kind;
  }

  function stepNeedsValue(kind: string): { needs: boolean; ph: string } {
    const m = STEP_META.find((s) => s.value === kind);
    return { needs: !!m?.needsVal, ph: m?.ph || '' };
  }

  function toggleEnabled(idx: number, val: boolean) {
    const next = flows.map((f, i) => (i === idx ? { ...f, enabled: val } : f));
    onSaveFlows(next);
  }

  function updateFlowHotkey(idx: number, spec: string) {
    const next = flows.map((f, i) => (i === idx ? { ...f, hotkey: spec.trim() } : f));
    onSaveFlows(next);
  }

  function removeFlow(idx: number) {
    const next = flows.filter((_, i) => i !== idx);
    onSaveFlows(next);
  }

  function addDraftStep() {
    if (draftSteps.length >= 12) return;
    draftSteps = [...draftSteps, { kind: 'trim', value: '' }];
  }

  function updateDraftStep(idx: number, patch: Partial<WorkflowStep>) {
    draftSteps = draftSteps.map((s, i) => (i === idx ? { ...s, ...patch } : s));
  }

  function moveDraftStep(idx: number, delta: number) {
    const to = idx + delta;
    if (to < 0 || to >= draftSteps.length) return;
    const copy = [...draftSteps];
    const [picked] = copy.splice(idx, 1);
    copy.splice(to, 0, picked!);
    draftSteps = copy;
  }

  function removeDraftStep(idx: number) {
    if (draftSteps.length <= 1) return;
    draftSteps = draftSteps.filter((_, i) => i !== idx);
  }

  function editFlow(f: WorkflowDef) {
    editingId = f.id;
    draftName = f.name; draftKeyword = f.keyword; draftDesc = f.description;
    draftSteps = f.steps.length ? f.steps.map((s) => ({ kind: s.kind, value: s.value || '' })) : [{ kind: 'read_clipboard', value: '' }];
  }

  function runDry(f: WorkflowDef) {
    postHost({ cmd: 'dry_run_flow', flow: $state.snapshot(f), sample_clip: drySample });
  }

  function createFlow() {
    const name = draftName.trim();
    if (!name || draftSteps.length === 0) return;
    const baseKw = (draftKeyword.trim() || name).toLowerCase().replace(/[^a-z0-9_-]/g, '');
    const id = editingId || `custom_${baseKw || 'flow'}_${Date.now().toString(36).slice(-4)}`;
    const prev = flows.find((f) => f.id === editingId);
    const nextFlow: WorkflowDef = {
      id, name, keyword: draftKeyword.trim().toLowerCase(), hotkey: prev?.hotkey || '',
      description: draftDesc.trim(), enabled: prev ? prev.enabled : true,
      steps: draftSteps.map((s) => ({ kind: s.kind, value: (s.value || '').trim() })),
    };
    onSaveFlows(editingId ? flows.map((f) => (f.id === editingId ? nextFlow : f)) : [...flows, nextFlow]);
    editingId = null; draftName = ''; draftKeyword = ''; draftDesc = '';
  }
  function saveSnippet() {
    const k = sKw.trim(), t = sTpl.trim();
    if (!k || !t || !onSaveSnippets) return;
    const row: SnippetEntry = { keyword: k, title: sTitle.trim() || k, tags: sTags.split(',').map((x) => x.trim()).filter(Boolean), template: t };
    onSaveSnippets(sEdit ? snippets.map((s) => (s.keyword === sEdit ? row : s)) : [...snippets.filter((s) => s.keyword.toLowerCase() !== k.toLowerCase()), row]);
    sEdit = null; sKw = ''; sTitle = ''; sTags = ''; sTpl = '';
  }
</script>

{#if sub === 'mcp'}
  <McpServersCard {lang} servers={mcpServers} {mcpPath} probingName={probingMcp} probeResult={mcpProbe} onSaveServers={(n) => onSaveMcp?.(n)} onProbeServer={(s) => onProbeMcp?.(s)} />
{:else if sub === 'snippets'}
  <div class="tab-head-bar">
    <div><h2>{isEn ? 'Snippets (snip)' : '快捷短语与模板 (snip)'}</h2><p class="tab-lead">{isEn ? 'Portable snippets.toml with {{date}} / {{uuid}} / {{clipboard}}.' : '纯文本 snippets.toml 存储，支持 {{date}} / {{uuid}} / {{clipboard}} 变量。'}</p></div>
    {#if snippetsPath}<button type="button" class="t-chip" onclick={() => postHost({ cmd: 'open_path', path: snippetsPath })}>snippets.toml</button>{/if}
  </div>
  <div class="t-card flow-list">
    {#if snippets.length === 0}<div class="empty-msg">{isEn ? 'No snippets yet.' : '暂无短语，可在下方录入。'}</div>
    {:else}{#each snippets as s}
      <div class="flow-item"><div class="flow-row">
        <div class="flow-main"><div class="flow-top"><span class="t-pill">{s.keyword}</span><span class="t-label">{s.title}</span>{#each s.tags || [] as tg}<span class="t-pill">#{tg}</span>{/each}</div><div class="t-desc mono">{s.template}</div></div>
        <div class="flow-ops"><button type="button" class="hk-pill" onclick={() => { sEdit = s.keyword; sKw = s.keyword; sTitle = s.title; sTags = (s.tags || []).join(', '); sTpl = s.template; }}>{isEn ? 'Edit' : '编辑'}</button><button type="button" class="hk-pill" onclick={() => onSaveSnippets?.(snippets.filter((x) => x.keyword !== s.keyword))}>×</button></div>
      </div></div>
    {/each}{/if}
  </div>
  <div class="t-card creator" style="margin-top:14px">
    <div class="creator-title">{sEdit ? (isEn ? `Edit: ${sEdit}` : `编辑：${sEdit}`) : (isEn ? 'Add Snippet' : '快速录入短语')}</div>
    <div class="creator-grid">
      <input class="t-chip" style="text-align:left;cursor:text" value={sKw} oninput={(e) => (sKw = (e.currentTarget as HTMLInputElement).value)} placeholder={isEn ? 'Keyword (sig)' : '触发词 (sig)'} />
      <input class="t-chip" style="text-align:left;cursor:text" value={sTitle} oninput={(e) => (sTitle = (e.currentTarget as HTMLInputElement).value)} placeholder={isEn ? 'Title' : '标题'} />
    </div>
    <input class="t-chip" style="text-align:left;cursor:text" value={sTags} oninput={(e) => (sTags = (e.currentTarget as HTMLInputElement).value)} placeholder={isEn ? 'Tags (mail,work)' : '标签 (逗号分隔)'} />
    <input class="t-chip mono" style="text-align:left;cursor:text" value={sTpl} oninput={(e) => (sTpl = (e.currentTarget as HTMLInputElement).value)} placeholder={isEn ? 'Template body' : '模板 ({{date}} / {{clipboard}})'} />
    <div class="chip-grid">{#each ['{{date}}', '{{datetime}}', '{{unix}}', '{{uuid}}', '{{clipboard}}'] as v}<button type="button" class="hk-pill" onclick={() => (sTpl = sTpl ? `${sTpl} ${v}` : v)}>+{v}</button>{/each}</div>
    <div class="creator-foot">
      {#if sEdit}<button type="button" class="hk-pill" onclick={() => { sEdit = null; sKw = ''; sTitle = ''; sTags = ''; sTpl = ''; }}>{isEn ? 'Cancel' : '取消'}</button>{:else}<span></span>{/if}
      <button type="button" class="hk-pill is-bound" disabled={!sKw.trim() || !sTpl.trim()} onclick={saveSnippet}>{sEdit ? (isEn ? 'Update' : '保存') : (isEn ? 'Add' : '录入并重载')}</button>
    </div>
  </div>
{:else}
<div class="tab-head">
  <div class="head-bar">
    <div>
      <h2>{tr(lang, 'h_flows')}</h2>
      <p class="tab-lead">{tr(lang, 'lead_flows')}</p>
    </div>
    {#if flowsPath}
      <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'open_path', path: flowsPath })}>
        <Icon name="FileCode" size={13} />
        flows.toml
      </Button>
    {/if}
  </div>
</div>

<div class="t-card flow-list">
  {#if flows.length === 0}
    <div class="empty-msg">{isEn ? 'No workflows configured yet.' : '暂无工作流，可在下方创建自定义动作管道。'}</div>
  {:else}
    {#each flows as f, i}
      {@const editingHk = hkEditId === f.id}
      <div class="flow-item">
        <div class="flow-row {f.enabled ? '' : 'is-off'}">
          <div class="flow-main">
            <div class="flow-top">
              <Icon name="Workflow" size={14} class="flow-ico" />
              <span class="t-label">{f.name}</span>
              {#if f.keyword}<span class="t-pill">{f.keyword}</span>{/if}
              <button
                type="button"
                class="hk-pill {f.hotkey || editingHk ? 'is-bound' : ''}"
                data-flow-hk={f.id}
                onclick={() => (hkEditId = editingHk ? null : f.id)}
              >
                <Icon name="Keyboard" size={11} />
                <span>{f.hotkey || (isEn ? 'Bind Hotkey' : '绑定全局热键')}</span>
              </button>
            </div>
            {#if f.description}<div class="t-desc">{f.description}</div>{/if}
            <div class="step-chain">
              {#each f.steps as s, si}
                {#if si > 0}<span class="arrow">→</span>{/if}
                <span class="step-chip">{stepLabel(s.kind)}</span>
              {/each}
            </div>
          </div>
          <div class="flow-ops">
            <button type="button" class="hk-pill" onclick={() => { dryId = dryId === f.id ? null : f.id; if (dryId === f.id) runDry(f); }}>{isEn ? 'Dry' : '试跑'}</button>
            <button type="button" class="hk-pill" onclick={() => editFlow(f)}>{isEn ? 'Edit' : '编辑'}</button>
            <Switch checked={f.enabled} ariaLabel={f.name} onchange={(v) => toggleEnabled(i, v)} />
            <Button variant="ghost" size="sm" onclick={() => removeFlow(i)}>
              <Icon name="Trash2" size={13} />
            </Button>
          </div>
        </div>
        {#if dryId === f.id}
          <div class="hk-drawer" style="flex-direction:column;align-items:stretch;gap:6px">
            <div style="display:flex;align-items:center;gap:8px">
              <input class="t-chip" style="flex:1;text-align:left;cursor:text" value={drySample} oninput={(e) => (drySample = (e.currentTarget as HTMLInputElement).value)} placeholder={isEn ? 'Sample clipboard…' : '测试输入/剪贴板…'} />
              <button type="button" class="hk-pill is-bound" onclick={() => runDry(f)}>{isEn ? 'Run' : '执行'}</button>
            </div>
            {#if dryRes && dryRes.flow_id === f.id}
              <div class="t-desc mono">
                {#each dryRes.trace || [] as t}<div>[{t.index}] {t.kind} → {t.output}</div>{/each}
                {#if (dryRes.effects || []).length > 0}<div>{isEn ? 'Skipped:' : '预期副作用:'} {(dryRes.effects || []).join(' · ')}</div>{/if}
              </div>
            {/if}
          </div>
        {/if}
        {#if editingHk}
          <div class="hk-drawer" data-flow-hk-drawer={f.id}>
            <div class="t-desc">{isEn ? 'Triggers this workflow directly without opening launcher (auto-avoids fullscreen apps).' : '按下即直接静默触发该工作流（自动继承全屏/远程桌面避让与冲突接管）。'}</div>
            <HotkeyRecorder
              value={f.hotkey || ''}
              presets={FLOW_HK_PRESETS}
              allowDisable={true}
              presetPlaceholder={isEn ? 'Preset' : '预设'}
              disabledLabel={isEn ? 'Unbound' : '未绑定'}
              onchange={(spec) => updateFlowHotkey(i, spec)}
            />
          </div>
        {/if}
      </div>
    {/each}
  {/if}
</div>

<div class="t-card creator" style="margin-top:14px">
  <div class="creator-title">{isEn ? 'Create Declarative Action Flow' : '新建声明式工作流（零脚本运行时 · 自动同步）'}</div>
  <div class="creator-grid">
    <Input size="sm" bind:value={draftName} placeholder={isEn ? 'Flow name (e.g. Clean & Copy)' : '工作流名称（如 剪贴板大写复制）'} />
    <Input size="sm" bind:value={draftKeyword} placeholder={isEn ? 'Trigger keyword (optional, e.g. upper)' : '快捷触发词（可选，如 upper）'} />
  </div>
  <Input size="sm" bind:value={draftDesc} placeholder={isEn ? 'Short description (optional)' : '简要说明（可选）'} />

  <div class="steps-editor">
    {#each draftSteps as step, idx}
      {@const info = stepNeedsValue(step.kind)}
      <div class="step-edit-row">
        <span class="step-num">#{idx + 1}</span>
        <Select
          size="sm"
          minWidth="156px"
          value={step.kind}
          options={stepOptions}
          onchange={(k) => updateDraftStep(idx, { kind: k })}
        />
        {#if info.needs}
          <div class="step-val">
            <Input
              size="sm"
              value={step.value || ''}
              placeholder={info.ph}
              oninput={(v) => updateDraftStep(idx, { value: v })}
            />
          </div>
        {/if}
        {#if draftSteps.length > 1}
          <button type="button" class="hk-pill" disabled={idx === 0} onclick={() => moveDraftStep(idx, -1)}>↑</button>
          <button type="button" class="hk-pill" disabled={idx === draftSteps.length - 1} onclick={() => moveDraftStep(idx, 1)}>↓</button>
          <Button variant="ghost" size="sm" onclick={() => removeDraftStep(idx)}>
            <Icon name="Trash2" size={12} />
          </Button>
        {/if}
      </div>
    {/each}
  </div>

  <div class="creator-foot">
    <Button variant="secondary" size="sm" onclick={addDraftStep}>
      <Icon name="Plus" size={13} />
      {isEn ? 'Add Step' : '追加步骤'}
    </Button>
    <Button variant="primary" size="sm" class="btn-add-flow" disabled={!draftName.trim()} onclick={createFlow}>
      <Icon name="Plus" size={13} />
      {isEn ? 'Save Workflow' : '保存并热重载'}
    </Button>
  </div>
</div>
{/if}

<style>
  .head-bar { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .flow-list { padding: 8px 16px; }
  .empty-msg { padding: 14px 0; font-size: 12.5px; color: var(--muted); }
  .flow-item { border-bottom: 1px solid var(--border); }
  .flow-item:last-child { border-bottom: 0; }
  .flow-row {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 12px 0;
  }
  .flow-row.is-off { opacity: 0.55; }
  .hk-pill {
    display: inline-flex; align-items: center; gap: 4px; padding: 2px 7px; border-radius: 999px;
    border: 1px solid var(--border-strong); background: var(--surface); color: var(--muted);
    font-family: inherit; font-size: 11px; font-weight: 500; cursor: pointer;
    transition: border-color 0.12s ease, color 0.12s ease, background-color 0.12s ease;
  }
  .hk-pill:hover, .hk-pill.is-bound { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .hk-drawer {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    margin: 0 0 10px; padding: 9px 12px; border-radius: 8px;
    background: var(--canvas); border: 1px solid var(--border);
  }
  .flow-main { min-width: 0; flex: 1; }
  .flow-top { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  :global(.flow-ico) { color: var(--accent); flex-shrink: 0; }
  .step-chain { display: flex; align-items: center; gap: 5px; flex-wrap: wrap; margin-top: 7px; }
  .step-chip {
    font-size: 11px; padding: 2px 7px; border-radius: 999px;
    background: var(--canvas); border: 1px solid var(--border); color: var(--text-secondary);
  }
  .arrow { font-size: 11px; color: var(--muted); }
  .flow-ops { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  .creator { padding: 14px 16px; display: flex; flex-direction: column; gap: 10px; }
  .creator-title { font-size: 13px; font-weight: 600; color: var(--text); }
  .creator-grid { display: grid; grid-template-columns: 1.3fr 1fr; gap: 8px; }
  .steps-editor { display: flex; flex-direction: column; gap: 7px; padding-top: 4px; }
  .step-edit-row { display: flex; align-items: center; gap: 8px; }
  .step-num { font-size: 11.5px; font-weight: 600; color: var(--muted); width: 22px; }
  .step-val { flex: 1; min-width: 0; }
  .creator-foot { display: flex; align-items: center; justify-content: space-between; padding-top: 4px; }
</style>
