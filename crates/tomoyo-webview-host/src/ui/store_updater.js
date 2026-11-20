    // --- Plugin Store & GitHub Releases Auto-Updater ---
    let storeItems = [];

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

    function checkUpdatesNow() {
      const btn = document.getElementById('btn-check-update');
      const box = document.getElementById('update-result-box');
      const repo = (document.getElementById('upd_repo').value || '').trim() || 'q871958549/Tomoyo';
      btn.disabled = true;
      box.style.display = 'block';
      box.textContent = t('upd_checking');
      post({ cmd: 'check_updates', repo });
    }

    function openRepo() {
      const repo = (document.getElementById('upd_repo').value || '').trim() || 'q871958549/Tomoyo';
      post({ cmd: 'open_external', url: 'https://github.com/' + repo });
    }

    function renderUpdateStatus(st) {
      const btn = document.getElementById('btn-check-update');
      const box = document.getElementById('update-result-box');
      btn.disabled = false;
      box.style.display = 'block';
      if (!st) return;
      if (st.state === 'available' && st.release) {
        const r = st.release;
        const dl = r.download_url || r.html_url;
        box.innerHTML = `
          <div style="display:flex;align-items:center;justify-content:space-between;gap:12px">
            <div><b>${esc(t('upd_avail')(r.version))}</b> <span style="color:var(--muted);font-size:12px">${esc(r.name)}</span></div>
            <button class="btn primary" onclick="post({cmd:'open_external',url:${JSON.stringify(dl)}})">${esc(t('upd_download'))}</button>
          </div>
          ${r.notes ? `<div class="update-notes">${esc(r.notes)}</div>` : ''}
        `;
      } else if (st.state === 'up_to_date') {
        const extra = st.note ? `<div class="label-desc" style="margin-top:4px">${esc(st.note)}</div>` : '';
        box.innerHTML = `<div style="color:var(--ok);font-weight:600">${esc(t('upd_latest')(st.current || INIT.version || '0.6.0'))}</div>${extra}`;
      } else if (st.state === 'error') {
        box.innerHTML = `<div style="color:#ef4444;font-weight:500">${esc(t('upd_err')(st.message || 'Unknown error'))}</div>`;
      }
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
        } else if (data.event === 'store_loaded') {
          storeItems = data.items || [];
          const srcs = (data.sources || []).join(' · ');
          const errs = (data.errors || []).length ? ` (${data.errors.join('; ')})` : '';
          document.getElementById('store-status').textContent = (srcs || 'Registry') + errs;
          renderStoreList();
        } else if (data.event === 'store_action_result') {
          document.getElementById('toast').textContent = (data.ok ? '✓ ' : '✗ ') + data.message;
          if (data.ok) fetchStore();
        }
      });
    }
