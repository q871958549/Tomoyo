<script lang="ts">
  import Icon from '../../lib/ui/Icon.svelte';
  import { postHost } from '../../lib/ipc';
  import type { Lang } from '../../lib/i18n';
  import type { McpProbeResult, McpServerEntry } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Input from '../../lib/ui/Input.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    lang: Lang;
    servers: McpServerEntry[];
    mcpPath: string;
    probingName: string;
    probeResult: McpProbeResult | null;
    onSaveServers: (next: McpServerEntry[]) => void;
    onProbeServer: (srv: McpServerEntry) => void;
  }

  let { lang, servers, mcpPath, probingName, probeResult, onSaveServers, onProbeServer }: Props = $props();
  let isEn = $derived(lang === 'en');
  let name = $state(''), command = $state(''), argsStr = $state(''), envStr = $state(''), editing = $state<string | null>(null);

  function splitArgs(raw: string): string[] {
    const t = raw.trim();
    if (!t) return [];
    if (t.startsWith('[')) { try { const p = JSON.parse(t); if (Array.isArray(p)) return p.map(String).filter(Boolean); } catch {} }
    return t.split(/\s+/).filter(Boolean);
  }
  function parseEnv(raw: string): Record<string, string> {
    const out: Record<string, string> = {};
    for (const part of raw.split(/[;\n]+/)) {
      const idx = part.indexOf('=');
      if (idx > 0) out[part.slice(0, idx).trim()] = part.slice(idx + 1).trim();
    }
    return out;
  }
  function editServer(s: McpServerEntry) {
    editing = s.name; name = s.name; command = s.command;
    argsStr = (s.args || []).join(' ');
    envStr = Object.entries(s.env || {}).map(([k, v]) => `${k}=${v}`).join('; ');
  }
  function resetForm() { editing = null; name = ''; command = ''; argsStr = ''; envStr = ''; }
  function saveServer() {
    const n = name.trim(), c = command.trim();
    if (!n || !c) return;
    const prev = editing ? servers.find((s) => s.name.toLowerCase() === editing!.toLowerCase()) : undefined;
    const row: McpServerEntry = { name: n, command: c, args: splitArgs(argsStr), env: parseEnv(envStr), enabled: prev ? prev.enabled : true };
    const filtered = servers.filter((s) => s.name.toLowerCase() !== (editing || n).toLowerCase() && s.name.toLowerCase() !== n.toLowerCase());
    onSaveServers([...filtered, row]);
    resetForm();
  }
</script>

<div class="tab-head head-bar">
  <div>
    <h2>{isEn ? 'STDIO MCP Servers (mcp.toml)' : 'STDIO MCP 工具服务 (mcp.toml)'}</h2>
    <p class="tab-lead">{isEn ? 'Zero idle RAM: external MCP tools spawn strictly on demand via `mcp` or `dev.tomoyo.ai` workbench.' : '严守 0 MB 常驻：仅在调用 `mcp` 指令或打开 AI 工作台时按需拉起外部工具并握手。'}</p>
  </div>
  {#if mcpPath}
    <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'reveal_path', path: mcpPath })}>
      <Icon name="FolderOpen" size={13} />{isEn ? 'Reveal mcp.toml' : '定位 mcp.toml'}
    </Button>
  {/if}
</div>

<div class="t-card mcp-list">
  {#if servers.length === 0}
    <div class="empty-msg">{isEn ? 'No MCP servers configured yet. Add one below (e.g. npx @modelcontextprotocol/server-filesystem).' : '暂无 MCP 服务配置；可在下方添加（如 filesystem / github / sqlite 等 STDIO 服务）。'}</div>
  {:else}
    {#each servers as s (s.name)}
      <div class="mcp-item">
        <div class="mcp-row" class:is-off={!s.enabled}>
          <div class="mcp-main">
            <div class="mcp-top">
              <Icon name="Zap" size={14} class="mcp-ico" />
              <span class="t-label">{s.name}</span>
              <span class="t-pill mono">{s.command} {(s.args || []).join(' ')}</span>
              {#if Object.keys(s.env || {}).length > 0}
                <span class="t-pill">{Object.keys(s.env).length} env</span>
              {/if}
            </div>
            {#if probeResult && probeResult.name.toLowerCase() === s.name.toLowerCase()}
              <div class="probe-box" class:is-bad={!probeResult.ok}>
                <div class="probe-head">
                  <Icon name={probeResult.ok ? 'CheckCircle2' : 'AlertCircle'} size={13} />
                  <span>{probeResult.message}</span>
                  {#if probeResult.elapsed_ms !== undefined}<span class="t-pill">{probeResult.elapsed_ms} ms</span>{/if}
                </div>
                {#if probeResult.tools && probeResult.tools.length > 0}
                  <div class="tool-chips">
                    {#each probeResult.tools as tool (tool.name)}
                      <span class="t-pill mono" title={tool.description || ''}>{tool.name}</span>
                    {/each}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
          <div class="mcp-ops">
            <Button variant="secondary" size="sm" disabled={probingName === s.name} onclick={() => onProbeServer(s)} title={isEn ? 'Probe tools/list' : '实时握手探测 tools/list'}>
              <Icon name="RefreshCw" size={13} />{probingName === s.name ? (isEn ? 'Probing…' : '探测中…') : (isEn ? 'Probe' : '测试连通')}
            </Button>
            <Button variant="ghost" size="sm" onclick={() => editServer(s)} title={isEn ? 'Edit' : '编辑'}>
              <Icon name="Pencil" size={13} />
            </Button>
            <Switch checked={s.enabled} onchange={(v) => onSaveServers(servers.map((r) => (r.name === s.name ? { ...r, enabled: v } : r)))} />
            <Button variant="danger" size="sm" onclick={() => onSaveServers(servers.filter((r) => r.name !== s.name))} title={isEn ? 'Delete' : '删除'}>
              <Icon name="Trash2" size={13} />
            </Button>
          </div>
        </div>
      </div>
    {/each}
  {/if}
</div>

<div class="t-card mcp-form" style="margin-top: 12px;">
  <div class="form-title">{editing ? (isEn ? `Edit MCP Server: ${editing}` : `正在编辑服务：${editing}`) : (isEn ? 'Add STDIO MCP Server' : '新增 STDIO MCP 服务')}</div>
  <div class="form-grid">
    <Input size="sm" bind:value={name} placeholder={isEn ? 'Server name (e.g. filesystem)' : '服务名（如 filesystem / sqlite）'} />
    <Input size="sm" mono bind:value={command} placeholder={isEn ? 'Executable (e.g. npx / uvx / python)' : '命令（如 npx / uvx / tomoyo-plugin-ai）'} />
  </div>
  <div class="form-grid">
    <Input size="sm" mono bind:value={argsStr} placeholder={isEn ? 'Args (space separated)' : '启动参数（空格分隔，如 -y @modelcontextprotocol/server-filesystem /docs）'} />
    <Input size="sm" mono bind:value={envStr} placeholder={isEn ? 'Env KEY=VAL; KEY2=VAL2 (optional)' : '环境变量 KEY=VAL; KEY2=VAL2（可选）'} />
  </div>
  <div class="form-foot">
    <div>
      {#if editing}
        <Button variant="ghost" size="sm" onclick={resetForm}>{isEn ? 'Cancel' : '取消编辑'}</Button>
      {/if}
    </div>
    <Button variant="primary" size="sm" disabled={!name.trim() || !command.trim()} onclick={saveServer}>
      <Icon name="Plus" size={13} />{editing ? (isEn ? 'Update & Save' : '保存修改') : (isEn ? 'Save Server' : '添加并刷新缓存')}
    </Button>
  </div>
</div>

<style>
  .head-bar { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .mcp-list { padding: 8px 16px; }
  .empty-msg { padding: 14px 0; font-size: 12.5px; color: var(--muted); }
  .mcp-item { border-bottom: 1px solid var(--border); }
  .mcp-item:last-child { border-bottom: 0; }
  .mcp-row { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; padding: 12px 0; }
  .mcp-row.is-off { opacity: 0.55; }
  .mcp-main { min-width: 0; flex: 1; }
  .mcp-top { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  :global(.mcp-ico) { color: var(--accent); flex-shrink: 0; }
  .mcp-ops { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  .probe-box { margin-top: 8px; padding: 8px 10px; border-radius: 8px; background: var(--ok-soft); border: 1px solid var(--border); font-size: 12px; }
  .probe-box.is-bad { background: var(--danger-soft); color: var(--danger); }
  .probe-head { display: flex; align-items: center; gap: 6px; font-weight: 500; }
  .tool-chips { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 6px; }
  .mcp-form { padding: 14px 16px; display: flex; flex-direction: column; gap: 9px; }
  .form-title { font-size: 13px; font-weight: 600; color: var(--text); }
  .form-grid { display: grid; grid-template-columns: 1fr 1.4fr; gap: 8px; }
  .form-foot { display: flex; align-items: center; justify-content: space-between; }
</style>
