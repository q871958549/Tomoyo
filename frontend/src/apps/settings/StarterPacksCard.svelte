<script lang="ts">
  import { Check, Download } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import type { Lang } from '../../lib/i18n';
  import Button from '../../lib/ui/Button.svelte';
  import type { StoreItem } from './StoreTab.svelte';

  interface PackDef {
    id: string;
    zh: string;
    en: string;
    descZh: string;
    descEn: string;
    ids: string[];
  }

  const PACKS: PackDef[] = [
    {
      id: 'pure-core',
      zh: '纯净微内核',
      en: 'Pure Microkernel',
      descZh: '仅启动器、文件检索、计算器与工作流，0 插件零常驻',
      descEn: 'Launcher, file search, calculator & flows only (zero plugins)',
      ids: [],
    },
    {
      id: 'productivity',
      zh: '高频效率套件',
      en: 'Productivity Pack',
      descZh: '剪贴板工作台 + 原生截图录屏 + 多源划词翻译 + 窗口布局',
      descEn: 'Clipboard workbench + native capture + translator + WinLayout',
      ids: ['dev.tomoyo.clipboard', 'dev.tomoyo.capture', 'dev.tomoyo.translate', 'dev.tomoyo.winlayout'],
    },
    {
      id: 'writing-ocr',
      zh: '写作识字套件',
      en: 'Writing & OCR Pack',
      descZh: '离线 OCR 识字 + Quick Notes 备忘录 + Quiet.md',
      descEn: 'Offline OCR + Quick Notes + Quiet.md Markdown editor',
      ids: ['dev.tomoyo.ocr', 'dev.tomoyo.notes', 'dev.tomoyo.markdown-editor'],
    },
    {
      id: 'full-suite',
      zh: '官方全家桶',
      en: 'Full Official Suite',
      descZh: '全部 9 款官方 STDIO 与纯 WASM 插件一键配齐，均可单独卸载',
      descEn: 'All 9 standalone official STDIO & WASM plugins, each removable',
      ids: [
        'dev.tomoyo.clipboard',
        'dev.tomoyo.capture',
        'dev.tomoyo.translate',
        'dev.tomoyo.ocr',
        'dev.tomoyo.notes',
        'dev.tomoyo.markdown-editor',
        'dev.tomoyo.winlayout',
        'dev.tomoyo.devtools',
        'dev.tomoyo.regex',
      ],
    },
  ];

  interface Props {
    lang: Lang;
    catalogById: Record<string, StoreItem>;
    installedIds: string[];
  }

  let { lang, catalogById, installedIds }: Props = $props();
  let isEn = $derived(lang === 'en');
  let installedSet = $derived(new Set(installedIds.map((x) => x.toLowerCase())));

  function isInstalled(id: string): boolean {
    const low = id.toLowerCase();
    return installedSet.has(low) || !!catalogById[low]?.installed;
  }

  function packStats(pack: PackDef) {
    let installedCount = 0;
    const missing: StoreItem[] = [];
    let unsupported = 0;
    for (const id of pack.ids) {
      if (isInstalled(id)) {
        installedCount += 1;
        continue;
      }
      const item = catalogById[id.toLowerCase()];
      if (!item) continue;
      if (item.platform_compatible === false) unsupported += 1;
      else if (item.source) missing.push(item);
    }
    return { installedCount, missing, unsupported, total: pack.ids.length };
  }

  function installPack(pack: PackDef) {
    const { missing } = packStats(pack);
    for (const item of missing) {
      postHost({ cmd: 'install_plugin', plugin_id: item.id, source: item.source, sha256: item.sha256 });
    }
  }
</script>

<div class="t-card packs-card" data-starter-packs>
  <div class="packs-head">
    <div>
      <div class="t-label">{isEn ? 'Starter Packs (On-Demand Setup)' : '按需装机套件 (Starter Packs)'}</div>
      <div class="t-desc">
        {isEn
          ? 'Core stays pure and lightweight. Pick any official bundle below — every plugin installs via verified SHA-256 and can be removed anytime.'
          : '核心保持纯净微内核；按需勾选官方组合包，所有插件均经 SHA-256 验签安装并可随时单独卸载。'}
      </div>
    </div>
  </div>
  <div class="packs-grid">
    {#each PACKS as pack}
      {@const st = packStats(pack)}
      <div class="pack-item" data-pack={pack.id}>
        <div class="pack-top">
          <span class="pack-name">{isEn ? pack.en : pack.zh}</span>
          {#if st.total > 0}
            <span class="t-pill {st.installedCount === st.total ? 'is-ok' : ''}">{st.installedCount}/{st.total}</span>
          {:else}
            <span class="t-pill is-ok">0 {isEn ? 'plugins' : '插件'}</span>
          {/if}
        </div>
        <div class="pack-desc">{isEn ? pack.descEn : pack.descZh}</div>
        <div class="pack-foot">
          {#if st.total === 0}
            <span class="pack-status">
              <Check size={12} />
              {installedIds.length === 0
                ? (isEn ? 'Pure Core Active' : '纯净微内核运行中')
                : (isEn ? 'Zero Core Bloat' : '核心零业务捆绑')}
            </span>
          {:else if st.installedCount === st.total}
            <span class="pack-status"><Check size={12} />{isEn ? 'All Installed' : '已全部配齐'}</span>
          {:else if st.missing.length > 0}
            <Button variant="secondary" size="sm" onclick={() => installPack(pack)}>
              <Download size={12} />
              {isEn ? `Install Missing (${st.missing.length})` : `一键补装 (${st.missing.length})`}
            </Button>
          {:else if st.unsupported > 0}
            <span class="t-desc">{isEn ? 'Some plugins unsupported on this OS' : '含当前系统暂不支持的插件'}</span>
          {:else}
            <span class="t-desc">{isEn ? 'Refresh catalog to resolve packages' : '刷新市场索引后可一键补装'}</span>
          {/if}
        </div>
      </div>
    {/each}
  </div>
</div>

<style>
  .packs-card { padding: 12px 14px; margin-bottom: 12px; display: flex; flex-direction: column; gap: 10px; }
  .packs-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 10px; }
  .packs-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .pack-item {
    display: flex; flex-direction: column; gap: 5px;
    padding: 10px 12px; border-radius: 9px;
    border: 1px solid var(--border); background: var(--canvas);
  }
  .pack-top { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .pack-name { font-size: 12.5px; font-weight: 600; color: var(--text); }
  .pack-desc { font-size: 11.5px; color: var(--text-secondary); line-height: 1.45; flex: 1; }
  .pack-foot { display: flex; align-items: center; justify-content: flex-end; margin-top: 2px; min-height: 26px; }
  .pack-status { display: inline-flex; align-items: center; gap: 4px; font-size: 11.5px; font-weight: 500; color: var(--ok); }
  .is-ok { background: var(--ok-soft); color: var(--ok); }
</style>
