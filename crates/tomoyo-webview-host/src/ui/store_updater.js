    // --- Plugin Store & GitHub Releases Auto-Updater ---
    let storeItems = [];
    let latestAvailableRelease = null;

    function fetchStore() {
      const st = document.getElementById('store-status');
      st.textContent = t('store_loading');
      const sources = state.plugin_registries && state.plugin_registries.length
        ? state.plugin_registries
        : ['registry/index.json'];
      post({ cmd: 'fetch_store', sources });
    }

    function renderStoreList() {
      const box = document.getElementById('store-list');
      box.innerHTML = '';
      if (!storeItems.length) {
        box.innerHTML = '<div class="label-desc">暂无可用插件条目</div>';
        return;
      }
      storeItems.forEach((p, idx) => {
        const div = document.createElement('div');
        div.className = 'store-item';
        const shaShort = (p.sha256 || '').slice(0, 12);
        let btnHtml = '';
        if (p.removable) {
          btnHtml = `<button class="btn danger" onclick="uninstallStorePlugin(${idx})">${esc(t('store_uninstall'))}</button>`;
        } else if (p.installed) {
          btnHtml = `<span class="pill">${esc(t('store_builtin'))}</span>`;
        } else {
          btnHtml = `<button class="btn primary" onclick="installStorePlugin(${idx})">${esc(t('store_install'))}</button>`;
        }
        div.innerHTML = `
          <div style="flex:1;min-width:0">
            <div class="label-title">${esc(p.name)} <span class="pill">${esc(p.runtime_kind)}</span> <span style="font-size:11.5px;color:var(--muted)">v${esc(p.version)}</span></div>
            <div class="label-desc">${esc(p.description)}</div>
            <div class="store-meta">
              <span>ID: ${esc(p.id)}</span>
              <span>Triggers: ${esc((p.keywords || []).join(', '))}</span>
              <span>SHA-256: ${esc(shaShort)}…</span>
            </div>
          </div>
          <div>${btnHtml}</div>
        `;
        box.appendChild(div);
      });
    }

    function installStorePlugin(idx) {
      const p = storeItems[idx];
      if (!p) return;
      post({ cmd: 'install_plugin', plugin_id: p.id, source: p.source, sha256: p.sha256 });
    }

    function uninstallStorePlugin(idx) {
      const p = storeItems[idx];
      if (!p) return;
      post({ cmd: 'uninstall_plugin', plugin_id: p.id });
    }

    function formatSourceLabel(src) {
      if (src === 'github_web') return t('upd_src_web');
      if (src === 'github_api_cached') return t('upd_src_cache');
      if (src === 'github_api') return t('upd_src_api');
      return '';
    }

    function formatUnixTime(sec) {
      if (!sec) return '';
      try { return new Date(sec * 1000).toLocaleString(); } catch (_) { return ''; }
    }

    function checkUpdatesNow() {
      const btn = document.getElementById('btn-check-update');
      const box = document.getElementById('update-result-box');
      const repo = (state.update && state.update.repo) || 'q871958549/Tomoyo';
      btn.disabled = true;
      box.style.display = 'block';
      box.textContent = t('upd_checking');
      post({ cmd: 'check_updates', repo });
    }

    function openRepo() {
      const repo = (state.update && state.update.repo) || 'q871958549/Tomoyo';
      post({ cmd: 'open_external', url: 'https://github.com/' + repo });
    }

    function openReleasesPage() {
      const repo = (state.update && state.update.repo) || 'q871958549/Tomoyo';
      post({ cmd: 'open_external', url: 'https://github.com/' + repo + '/releases' });
    }

    function renderUpdateStatus(st) {
      const btn = document.getElementById('btn-check-update');
      const box = document.getElementById('update-result-box');
      if (!btn || !box) return;
      btn.disabled = false;
      box.style.display = 'block';
      if (!st) return;
      const checkedStr = formatUnixTime(st.checked_at_unix);
      if (st.state === 'available' && st.release) {
        const r = st.release;
        latestAvailableRelease = r;
        const dl = r.download_url || r.html_url;
        const installBtn = r.download_url
          ? `<button class="btn primary" id="btn-install-upd" onclick="installAvailableUpdate()">${esc(t('upd_install_now'))}</button>`
          : '';
        const srcTag = formatSourceLabel(r.source || '');
        const metaParts = [srcTag, checkedStr ? t('upd_last_checked')(checkedStr) : ''].filter(Boolean);
        box.innerHTML = `
          <div style="display:flex;align-items:center;justify-content:space-between;gap:10px;flex-wrap:wrap">
            <div>
              <div><b>${esc(t('upd_avail')(r.version))}</b> <span style="color:var(--muted);font-size:12px">${esc(r.name)}</span></div>
              ${metaParts.length ? `<div class="upd-meta-line">${esc(metaParts.join(' · '))}</div>` : ''}
            </div>
            <div style="display:flex;gap:8px">
              <button class="btn" onclick="post({cmd:'open_external',url:${JSON.stringify(dl)}})">${esc(t('upd_download'))}</button>
              ${installBtn}
            </div>
          </div>
          <div id="upd-prog-wrap" class="upd-prog-wrap" style="display:none">
            <div id="upd-prog-txt" class="upd-meta-line"></div>
            <div class="upd-prog-track"><div id="upd-prog-fill" class="upd-prog-fill"></div></div>
          </div>
          ${r.notes ? `<div class="update-notes">${esc(r.notes)}</div>` : ''}
        `;
      } else if (st.state === 'up_to_date') {
        const meta = [st.note || '', checkedStr ? t('upd_last_checked')(checkedStr) : ''].filter(Boolean).join(' · ');
        const extra = meta ? `<div class="upd-meta-line">${esc(meta)}</div>` : '';
        box.innerHTML = `<div style="color:var(--ok);font-weight:600">${esc(t('upd_latest')(st.current || INIT.version || '1.5.0'))}</div>${extra}`;
      } else if (st.state === 'error') {
        box.innerHTML = `
          <div style="color:#ef4444;font-weight:500">${esc(t('upd_err')(st.message || 'Unknown error'))}</div>
          <div class="upd-err-actions">
            <button class="btn primary" onclick="checkUpdatesNow()">${esc(t('upd_retry'))}</button>
            <button class="btn" onclick="openReleasesPage()">${esc(t('upd_open_releases'))}</button>
          </div>
        `;
      }
    }

    function renderUpdateInstallProgress(p) {
      if (!p) return;
      const wrap = document.getElementById('upd-prog-wrap');
      const txt = document.getElementById('upd-prog-txt');
      const fill = document.getElementById('upd-prog-fill');
      if (wrap) wrap.style.display = 'block';
      if (txt) { txt.style.color = 'var(--text)'; txt.textContent = p.message || t('upd_downloading'); }
      if (fill) fill.style.width = Math.max(4, Math.min(100, Number(p.percent || 5))) + '%';
    }

    if (window.chrome && window.chrome.webview) {
      window.chrome.webview.addEventListener('message', (ev) => {
        let data = ev.data;
        if (typeof data === 'string') {
          try { data = JSON.parse(data); } catch (_) { return; }
        }
        if (!data || !data.event) return;
        if (data.event === 'update_status') {
          renderUpdateStatus(data.status);
        } else if (data.event === 'folder_picked') {
          if (typeof onFolderPicked === 'function') onFolderPicked(data);
        } else if (data.event === 'capture_dir_picked') {
          if (typeof onCaptureDirPicked === 'function') onCaptureDirPicked(data);
        } else if (data.event === 'settings_saved') {
          if (typeof onSettingsSavedAck === 'function') onSettingsSavedAck(data);
        } else if (data.event === 'store_loaded') {
          storeItems = data.items || [];
          const srcs = (data.sources || []).join(' · ');
          const errs = (data.errors || []).length ? ` (${data.errors.join('; ')})` : '';
          document.getElementById('store-status').textContent = (srcs || 'Registry') + errs;
          renderStoreList();
        } else if (data.event === 'store_action_result') {
          document.getElementById('toast').textContent = (data.ok ? '✓ ' : '✗ ') + data.message;
          if (data.ok) fetchStore();
        } else if (data.event === 'perm_action_result') {
          document.getElementById('toast').textContent = (data.ok ? '✓ ' : '✗ ') + data.message;
        } else if (data.event === 'logs_loaded') {
          renderLogsResult(data);
        } else if (data.event === 'diag_bundle_copied') {
          setToast(t('log_copied'), false);
        } else if (data.event === 'secrets_status') {
          if (typeof onSecretsStatusEvent === 'function') onSecretsStatusEvent(data);
        } else if (data.event === 'update_install_progress') {
          renderUpdateInstallProgress(data.progress);
        } else if (data.event === 'update_install_result') {
          const btn = document.getElementById('btn-install-upd');
          if (btn) {
            btn.disabled = !!data.ok;
            if (!data.ok) btn.textContent = t('upd_retrying_install');
          }
          const wrap = document.getElementById('upd-prog-wrap');
          const txt = document.getElementById('upd-prog-txt');
          const fill = document.getElementById('upd-prog-fill');
          if (wrap) wrap.style.display = 'block';
          if (txt) {
            txt.style.color = data.ok ? 'var(--ok)' : '#ef4444';
            txt.textContent = (data.ok ? '✓ ' : '✗ ') + data.message;
          }
          if (fill && data.ok) fill.style.width = '100%';
          document.getElementById('toast').textContent = (data.ok ? '✓ ' : '✗ ') + data.message;
          if (data.ok && data.launched_installer) {
            setTimeout(() => post({ cmd: 'close' }), 450);
          }
        }
      });
    }

    // --- Diagnostics & Log Traceability Viewer ---
    let curLogSource = 'tomoyo.log';

    window.addEventListener('error', (e) => {
      post({ cmd: 'report_js_error', message: String(e.message || 'error'), source: 'settings_ui', line: e.lineno || 0 });
    });
    window.addEventListener('unhandledrejection', (e) => {
      post({ cmd: 'report_js_error', message: String(e.reason || 'unhandledrejection'), source: 'settings_ui', line: 0 });
    });

    function setLogSource(src) {
      curLogSource = src;
      document.querySelectorAll('#logs-src-seg button').forEach(b => {
        b.classList.toggle('active', b.dataset.src === src);
      });
      refreshLogs();
    }

    function refreshLogs() {
      const pre = document.getElementById('logs-pre');
      if (pre) pre.textContent = t('log_loading');
      const errOnly = !!(document.getElementById('logs-err-only') && document.getElementById('logs-err-only').checked);
      post({ cmd: 'read_logs', source: curLogSource, errors_only: errOnly });
    }

    function renderLogsResult(data) {
      const meta = document.getElementById('logs-meta');
      const pre = document.getElementById('logs-pre');
      if (!meta || !pre) return;
      const iss = data.issues || {};
      const crashTag = iss.has_crash_report ? ' · 存在崩溃快照' : '';
      meta.textContent = `${data.source || curLogSource} · 近期 ERROR ${iss.errors || 0} / WARN ${iss.warnings || 0}${crashTag} · 目录：${data.logs_dir || INIT.logs_dir || ''}`;
      pre.textContent = data.content || '(空)';
      pre.scrollTop = pre.scrollHeight;
    }

    function copyDiagBundle() { post({ cmd: 'copy_diagnostic_bundle' }); }
    function openLogsDir() { post({ cmd: 'open_logs_dir' }); }

    function installAvailableUpdate() {
      if (!latestAvailableRelease) return;
      const btn = document.getElementById('btn-install-upd');
      if (btn) btn.disabled = true;
      renderUpdateInstallProgress({ message: t('upd_downloading'), percent: 3 });
      document.getElementById('toast').textContent = t('upd_downloading');
      post({ cmd: 'install_update', release: latestAvailableRelease });
    }
