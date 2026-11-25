<script lang="ts">
  import { CheckCircle2, Download, ExternalLink, RefreshCw, AlertCircle } from '@lucide/svelte';
  import tomoyoLogo from '../../assets/tomoyo-logo.png';
  import { postHost } from '../../lib/ipc';
  import { OFFICIAL_REPO, tr, type Lang } from '../../lib/i18n';
  import type { ReleaseInfo, TomoyoConfig, UpdateStatus } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Input from '../../lib/ui/Input.svelte';
  import Select from '../../lib/ui/Select.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    version: string;
    updateStatus: UpdateStatus | null;
    checking: boolean;
    pulse?: boolean;
    installProgress: { percent: number; message: string; done?: boolean; ok?: boolean } | null;
    onCheckNow: () => void;
    onInstall: (rel: ReleaseInfo) => void;
    onSave: (delayMs?: number) => void;
  }

  let {
    cfg,
    lang,
    version,
    updateStatus,
    checking,
    pulse = false,
    installProgress,
    onCheckNow,
    onInstall,
    onSave,
  }: Props = $props();

  let forceCustom = $state(false);
  let repoMode = $derived<'official' | 'custom'>(
    forceCustom || (cfg.update.repo || OFFICIAL_REPO) !== OFFICIAL_REPO ? 'custom' : 'official'
  );
  let customRepo = $state('');

  $effect(() => {
    const r = cfg.update.repo || OFFICIAL_REPO;
    if (r !== OFFICIAL_REPO) customRepo = r;
  });

  function isValidRepo(r: string): boolean {
    const m = r.trim().match(/^([A-Za-z0-9._-]{1,64})\/([A-Za-z0-9._-]{1,100})$/);
    return !!m && !m[1].includes('..') && !m[2].includes('..');
  }

  let customInvalid = $derived(repoMode === 'custom' && customRepo.trim().length > 0 && !isValidRepo(customRepo));

  const intervalOptions = [
    { value: '12', label: '12 h' },
    { value: '24', label: '24 h' },
    { value: '72', label: '72 h (3d)' },
    { value: '168', label: '168 h (7d)' },
  ];

  let repoOptions = $derived([
    { value: 'official', label: lang === 'en' ? `Official (${OFFICIAL_REPO})` : `官方源 (${OFFICIAL_REPO})` },
    { value: 'custom', label: lang === 'en' ? 'Custom Fork…' : '自定义 Fork 仓库…' },
  ]);

  function onRepoModeChange(v: string) {
    if (v === 'custom') {
      forceCustom = true;
    } else {
      forceCustom = false;
      customRepo = '';
      cfg.update.repo = OFFICIAL_REPO;
      onSave(0);
    }
  }

  function onCustomRepoInput(v: string) {
    customRepo = v;
    if (isValidRepo(v)) {
      cfg.update.repo = v.trim();
      onSave(260);
    }
  }

  function fmtTime(sec?: number): string {
    if (!sec) return '';
    try { return new Date(sec * 1000).toLocaleString(); } catch { return ''; }
  }

  function openRepo() {
    postHost({ cmd: 'open_external', url: `https://github.com/${cfg.update.repo || OFFICIAL_REPO}` });
  }
  function openReleases() {
    postHost({ cmd: 'open_external', url: `https://github.com/${cfg.update.repo || OFFICIAL_REPO}/releases` });
  }
</script>

<div class="tab-head">
  <h2>{tr(lang, 'h_updates')}</h2>
  <p class="tab-lead">{tr(lang, 'lead_updates')}</p>
</div>

<div class="t-card upd-hero">
  <div class="hero-top">
    <div class="hero-brand">
      <img class="hero-logo" src={tomoyoLogo} alt="Tomoyo" width="38" height="38" draggable="false" />
      <div>
        <div class="hero-title">
          <span>Tomoyo</span>
          <span class="t-pill">v{version}</span>
        </div>
        <div class="t-desc">{tr(lang, 'about_desc')}</div>
      </div>
    </div>
    <div class="hero-acts">
      <Button variant="secondary" onclick={openRepo}>
        <ExternalLink size={13} />
        {tr(lang, 'btn_github')}
      </Button>
      <Button variant="primary" disabled={checking} onclick={onCheckNow}>
        <span class="upd-ico {checking ? 'is-spinning' : ''}"><RefreshCw size={13} /></span>
        {checking ? (lang === 'en' ? 'Checking…' : '检查中…') : tr(lang, 'btn_check_upd')}
      </Button>
    </div>
  </div>

  {#if updateStatus}
    <div class="status-box {pulse ? 'is-fresh' : ''}">
      {#if updateStatus.state === 'up_to_date'}
        <div class="st-ok">
          <CheckCircle2 size={15} />
          <span>{lang === 'en' ? `Up to date (v${updateStatus.current || version})` : `当前已是最新版本 v${updateStatus.current || version}`}</span>
        </div>
        {#if updateStatus.checked_at_unix}
          <div class="t-desc" style="margin-top:4px">
            {updateStatus.note ? `${updateStatus.note} · ` : ''}{lang === 'en' ? 'Last checked:' : '上次检查：'} {fmtTime(updateStatus.checked_at_unix)}
          </div>
        {/if}
      {:else if updateStatus.state === 'available' && updateStatus.release}
        {@const rel = updateStatus.release}
        <div class="avail-row">
          <div>
            <div class="t-label">{lang === 'en' ? `New release v${rel.version}` : `发现新版本 v${rel.version}`}</div>
            <div class="t-desc">{rel.name}</div>
          </div>
          <div class="hero-acts">
            <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'open_external', url: rel.download_url || rel.html_url })}>
              {lang === 'en' ? 'Release Page' : '前往下载'}
            </Button>
            {#if rel.download_url}
              <Button variant="primary" size="sm" disabled={!!installProgress && !installProgress.done} onclick={() => onInstall(rel)}>
                <Download size={13} />
                {lang === 'en' ? 'Auto-Update & Restart' : '自动下载更新并重启'}
              </Button>
            {/if}
          </div>
        </div>
        {#if !rel.download_url}
          <div class="t-desc" style="margin-top:8px;color:var(--warn)">
            {lang === 'en'
              ? 'No prebuilt package for this OS was found in this GitHub Release yet; open Release Page to inspect assets.'
              : '该 Release 暂未包含当前系统的预编译更新包（仅找到其他平台安装包），请点击右侧「前往下载」查看或等待本平台产物发布。'}
          </div>
        {/if}
        {#if installProgress}
          <div class="prog-wrap">
            <div class="t-desc" style:color={installProgress.done && !installProgress.ok ? 'var(--danger)' : 'var(--text-secondary)'}>
              {installProgress.message}
            </div>
            <div class="prog-track">
              <div class="prog-fill" style:width="{Math.max(4, Math.min(100, installProgress.percent))}%"></div>
            </div>
          </div>
        {/if}
        {#if rel.notes}
          <div class="notes-box">{rel.notes}</div>
        {/if}
      {:else if updateStatus.state === 'error'}
        <div class="st-err">
          <AlertCircle size={15} />
          <span>{updateStatus.message || 'Check failed'}</span>
        </div>
        <div class="hero-acts" style="margin-top:10px">
          <Button variant="primary" size="sm" onclick={onCheckNow}>{lang === 'en' ? 'Retry' : '重试检查'}</Button>
          <Button variant="secondary" size="sm" onclick={openReleases}>{lang === 'en' ? 'GitHub Releases' : '前往 Releases'}</Button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<div class="t-card" style="margin-top:14px">
  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_upd_auto')}</div>
      <div class="t-desc">{tr(lang, 'desc_upd_auto')}</div>
    </div>
    <Switch checked={cfg.update.auto_check} onchange={(v) => { cfg.update.auto_check = v; onSave(0); }} />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{lang === 'en' ? 'Auto-download, verify & restart on new release' : '检测到新版本时自动更新并重启应用'}</div>
      <div class="t-desc">{lang === 'en' ? 'Automatically downloads the current OS package, verifies SHA-256, replaces binaries, and relaunches Tomoyo.' : '发现包含当前平台安装包的更新后，自动下载、校验 SHA-256、替换安装并重启 Tomoyo。'}</div>
    </div>
    <Switch checked={!!cfg.update.auto_install} onchange={(v) => { cfg.update.auto_install = v; onSave(0); }} />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_upd_interval')}</div>
      <div class="t-desc">{tr(lang, 'desc_upd_interval')}</div>
    </div>
    <Select
      value={String(cfg.update.check_interval_hours || 24)}
      options={intervalOptions}
      align="right"
      minWidth="124px"
      onchange={(v) => { cfg.update.check_interval_hours = Number(v) || 24; onSave(0); }}
    />
  </div>

  <div class="t-row col-row">
    <div class="hero-top" style="width:100%">
      <div>
        <div class="t-label">{tr(lang, 'lbl_upd_repo')}</div>
        <div class="t-desc">{tr(lang, 'desc_upd_repo')}</div>
      </div>
      <Select value={repoMode} options={repoOptions} align="right" minWidth="210px" onchange={onRepoModeChange} />
    </div>
    {#if repoMode === 'custom'}
      <div style="width:280px">
        <Input
          mono
          maxlength={165}
          bind:value={customRepo}
          placeholder="owner/repo"
          invalid={customInvalid}
          oninput={onCustomRepoInput}
        />
        {#if customInvalid}
          <div style="font-size:12px;color:var(--danger);margin-top:4px">格式需为 owner/repo</div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .upd-hero { padding: 16px; }
  .hero-top { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  .hero-brand { display: flex; align-items: center; gap: 12px; }
  .hero-logo { width: 38px; height: 38px; border-radius: 9px; display: block; flex-shrink: 0; filter: drop-shadow(0 1px 3px rgba(15, 23, 42, 0.16)); }
  .hero-title { display: flex; align-items: center; gap: 8px; font-size: 15px; font-weight: 600; }
  .hero-acts { display: flex; align-items: center; gap: 8px; }
  .status-box {
    margin-top: 14px; padding: 12px 14px; border-radius: 10px;
    background: var(--canvas); border: 1px solid var(--border);
    transition: border-color 0.2s ease, box-shadow 0.2s ease;
  }
  .status-box.is-fresh { border-color: var(--accent); box-shadow: 0 0 0 3px var(--ring); }
  .upd-ico { display: inline-flex; align-items: center; }
  .upd-ico.is-spinning { animation: tomoyo-upd-spin 0.8s linear infinite; }
  @keyframes tomoyo-upd-spin { to { transform: rotate(360deg); } }
  .st-ok { display: flex; align-items: center; gap: 6px; color: var(--ok); font-weight: 500; font-size: 13px; }
  .st-err { display: flex; align-items: center; gap: 6px; color: var(--danger); font-weight: 500; font-size: 13px; }
  .avail-row { display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap; }
  .prog-wrap { margin-top: 10px; }
  .prog-track { width: 100%; height: 6px; border-radius: 999px; background: var(--border); overflow: hidden; margin-top: 6px; }
  .prog-fill { height: 100%; border-radius: 999px; background: var(--accent); transition: width 0.16s ease; }
  .notes-box {
    margin-top: 10px; padding: 10px 12px; border-radius: 8px; background: var(--surface);
    border: 1px solid var(--border); font-size: 12px; color: var(--text-secondary);
    white-space: pre-wrap; max-height: 170px; overflow-y: auto;
  }
</style>
