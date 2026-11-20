    const INIT = window.__TOMOYO_SETTINGS_INIT__ || {};
    let state = JSON.parse(JSON.stringify(INIT.config || {}));
    let autostart = !!INIT.autostart;
    let recordingHotkey = false;
    let saveTimer = null;

    function curLang() {
      return (state.language || 'zh').toLowerCase().startsWith('en') ? 'en' : 'zh';
    }
    function t(key) {
      return (I18N[curLang()] && I18N[curLang()][key]) || I18N.zh[key] || key;
    }

    function post(msg) {
      if (window.chrome && window.chrome.webview) {
        window.chrome.webview.postMessage(JSON.stringify(msg));
      }
    }

    // Sidebar navigation
    let storeLoadedOnce = false;
    document.querySelectorAll('nav button').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('nav button').forEach(b => b.classList.remove('active'));
        document.querySelectorAll('section').forEach(s => s.classList.remove('active'));
        btn.classList.add('active');
        document.getElementById('tab-' + btn.dataset.tab).classList.add('active');
        if (btn.dataset.tab === 'store' && !storeLoadedOnce) {
          storeLoadedOnce = true;
          fetchStore();
        }
      });
    });

    function applyI18n() {
      const lang = curLang();
      document.documentElement.lang = lang === 'en' ? 'en' : 'zh-CN';
      document.getElementById('lang-zh').classList.toggle('active', lang === 'zh');
      document.getElementById('lang-en').classList.toggle('active', lang === 'en');
      document.querySelectorAll('[data-i18n]').forEach(el => {
        const k = el.getAttribute('data-i18n');
        const val = t(k);
        if (typeof val === 'string') el.textContent = val;
      });
      updateRowsLabel(document.getElementById('win_rows').value);
      updatePosDesc();
      renderWebRows();
      if (!recordingHotkey) renderHotkeyDisplay();
      document.getElementById('toast').textContent = t('toast_ready');
    }

    function setLanguage(code) {
      state.language = code;
      applyI18n();
      scheduleAutoSave(0);
    }

    function pickTheme(name, shouldSave) {
      state.theme = name;
      document.body.classList.remove('theme-dark', 'theme-midnight');
      if (name === 'dark') document.body.classList.add('theme-dark');
      else if (name === 'midnight') document.body.classList.add('theme-midnight');
      document.documentElement.style.colorScheme = name === 'light' ? 'light' : 'dark';
      document.querySelectorAll('.theme-card').forEach(c => {
        c.classList.toggle('selected', c.dataset.theme === name);
      });
      post({ cmd: 'sync_theme', theme: name });
      if (shouldSave) scheduleAutoSave(0);
    }

    function updateRowsLabel(v) {
      document.getElementById('win_rows-v').textContent = t('rows_unit')(v);
    }

    function updatePosDesc() {
      const w = state.window || {};
      const el = document.getElementById('pos-desc');
      if (w.custom_x != null && w.custom_y != null) {
        el.textContent = t('pos_saved')(w.custom_x, w.custom_y);
      } else {
        el.textContent = t('pos_auto');
      }
    }

    function resetPosition() {
      if (!state.window) state.window = {};
      state.window.custom_x = null;
      state.window.custom_y = null;
      updatePosDesc();
      scheduleAutoSave(0);
    }

    // --- Interactive Hotkey Recorder ---
    const recEl = document.getElementById('hotkey-recorder');
    function renderHotkeyDisplay() {
      const hk = state.hotkey || 'Alt+Space';
      recEl.classList.remove('recording');
      recEl.innerHTML = hk.split('+').map(p => `<kbd>${esc(p.trim())}</kbd>`).join(' + ');
    }
    function startHotkeyRecording() {
      recordingHotkey = true;
      recEl.classList.add('recording');
      recEl.textContent = t('rec_prompt');
      recEl.focus();
    }
    function stopHotkeyRecording() {
      recordingHotkey = false;
      renderHotkeyDisplay();
    }
    function applyHotkey(spec) {
      state.hotkey = spec;
      stopHotkeyRecording();
      scheduleAutoSave(0);
    }
    recEl.addEventListener('click', startHotkeyRecording);
    recEl.addEventListener('blur', () => { if (recordingHotkey) stopHotkeyRecording(); });

    function keyEventToName(e) {
      if (e.code === 'Space' || e.key === ' ') return 'Space';
      if (e.code === 'Tab') return 'Tab';
      if (e.code && e.code.startsWith('Key') && e.code.length === 4) return e.code.slice(3);
      if (e.code && e.code.startsWith('Digit') && e.code.length === 6) return e.code.slice(5);
      if (/^F([1-9]|1[0-2])$/.test(e.code || '')) return e.code;
      return null;
    }

    window.addEventListener('keydown', (e) => {
      if (recordingHotkey) {
        e.preventDefault();
        e.stopPropagation();
        if (e.key === 'Escape') {
          stopHotkeyRecording();
          return;
        }
        const mainKey = keyEventToName(e);
        if (!mainKey) return;
        const mods = [];
        if (e.ctrlKey) mods.push('Ctrl');
        if (e.altKey) mods.push('Alt');
        if (e.shiftKey) mods.push('Shift');
        if (e.metaKey) mods.push('Win');
        if (mods.length === 0) {
          recEl.textContent = t('rec_need_mod');
          return;
        }
        applyHotkey(mods.concat([mainKey]).join('+'));
        return;
      }
      if (e.key === 'Escape') {
        closeWin();
      }
    }, true);

    function renderWebRows() {
      const tb = document.getElementById('web-tbody');
      tb.innerHTML = '';
      (state.web_searches || []).forEach((w, idx) => {
        const tr = document.createElement('tr');
        tr.innerHTML = `
          <td><input type="text" value="${esc(w.keyword)}" style="width:74px" oninput="state.web_searches[${idx}].keyword=this.value;scheduleAutoSave(300)" /></td>
          <td><input type="text" value="${esc(w.name)}" style="width:130px" oninput="state.web_searches[${idx}].name=this.value;scheduleAutoSave(300)" /></td>
          <td><input type="text" value="${esc(w.url)}" style="width:100%" oninput="state.web_searches[${idx}].url=this.value;scheduleAutoSave(300)" /></td>
          <td><button class="btn danger" style="padding:4px 8px" onclick="delWebRow(${idx})">${esc(t('del'))}</button></td>
        `;
        tb.appendChild(tr);
      });
    }

    function addWebRow() {
      if (!state.web_searches) state.web_searches = [];
      state.web_searches.push({ keyword: 'wiki', name: 'Wikipedia', url: 'https://en.wikipedia.org/wiki/Special:Search?search={q}' });
      renderWebRows();
      scheduleAutoSave(0);
    }

    function delWebRow(i) {
      state.web_searches.splice(i, 1);
      renderWebRows();
      scheduleAutoSave(0);
    }

    function esc(s) {
      return String(s || '').replace(/&/g,'&amp;').replace(/"/g,'&quot;').replace(/</g,'&lt;');
    }

    function collectAndSave() {
      state.hibernate_delay_secs = parseInt(document.getElementById('hibernate').value, 10) || 15;
      state.auto_preview = document.getElementById('auto_preview').checked;
      state.show_filter_bar = document.getElementById('show_filter_bar').checked;
      state.window = {
        width_dip: parseInt(document.getElementById('win_width').value, 10) || 720,
        max_visible_rows: parseInt(document.getElementById('win_rows').value, 10) || 8,
        preview_width_dip: parseInt(document.getElementById('win_preview').value, 10) || 310,
        custom_x: state.window ? state.window.custom_x : null,
        custom_y: state.window ? state.window.custom_y : null
      };
      const rawDirs = (document.getElementById('apps_dirs').value || '')
        .split(/\r?\n|;/)
        .map(s => s.trim())
        .filter(Boolean);
      state.apps = {
        extra_dirs: rawDirs,
        auto_detect_desktop_organizers: document.getElementById('apps_org').checked,
        extra_scan_depth: parseInt(document.getElementById('apps_depth').value, 10) || 2
      };
      state.everything = {
        max_global_results: parseInt(document.getElementById('ev_global').value, 10) || 6,
        max_dedicated_results: parseInt(document.getElementById('ev_ded').value, 10) || 30,
        extra_filter: document.getElementById('ev_filter').value.trim()
      };
      state.clipboard = {
        persist_sqlite: document.getElementById('clip_persist').checked,
        max_persisted_items: parseInt(document.getElementById('clip_max').value, 10) || 500
      };
      const prevUpd = state.update || {};
      state.update = {
        auto_check: document.getElementById('upd_auto').checked,
        check_interval_hours: parseInt(document.getElementById('upd_interval').value, 10) || 24,
        repo: (document.getElementById('upd_repo').value || '').trim() || 'q871958549/Tomoyo',
        last_checked_unix: prevUpd.last_checked_unix || 0,
        skipped_version: prevUpd.skipped_version || null
      };
      post({ cmd: 'save_settings', config: state, autostart });
      document.getElementById('toast').textContent = t('toast_saved') + ' · ' + new Date().toLocaleTimeString();
    }

    function scheduleAutoSave(delayMs) {
      if (saveTimer) clearTimeout(saveTimer);
      if (delayMs <= 0) {
        collectAndSave();
      } else {
        saveTimer = setTimeout(collectAndSave, delayMs);
      }
    }

    function populate() {
      if (!state.language) state.language = 'zh';
      document.getElementById('about-ver').textContent = 'v' + (INIT.version || '0.6.0');
      document.getElementById('autostart').checked = autostart;
      const hib = state.hibernate_delay_secs || 15;
      document.getElementById('hibernate').value = hib;
      document.getElementById('hibernate-v').textContent = hib + ' s';
      document.getElementById('auto_preview').checked = state.auto_preview !== false;
      document.getElementById('show_filter_bar').checked = state.show_filter_bar !== false;

      pickTheme(state.theme || 'dark', false);

      const win = state.window || { width_dip: 720, max_visible_rows: 8, preview_width_dip: 310 };
      document.getElementById('win_width').value = win.width_dip || 720;
      document.getElementById('win_width-v').textContent = (win.width_dip || 720) + ' px';
      document.getElementById('win_rows').value = win.max_visible_rows || 8;
      document.getElementById('win_preview').value = win.preview_width_dip || 310;
      document.getElementById('win_preview-v').textContent = (win.preview_width_dip || 310) + ' px';

      const appsCfg = state.apps || {};
      document.getElementById('apps_org').checked = appsCfg.auto_detect_desktop_organizers !== false;
      document.getElementById('apps_depth').value = appsCfg.extra_scan_depth ?? 2;
      document.getElementById('apps_dirs').value = (appsCfg.extra_dirs || []).join('\n');

      const ev = state.everything || {};
      document.getElementById('ev_global').value = ev.max_global_results ?? 6;
      document.getElementById('ev_ded').value = ev.max_dedicated_results ?? 30;
      document.getElementById('ev_filter').value = ev.extra_filter || '';

      const cb = state.clipboard || {};
      document.getElementById('clip_persist').checked = cb.persist_sqlite !== false;
      document.getElementById('clip_max').value = cb.max_persisted_items ?? 500;

      const upd = state.update || {};
      document.getElementById('upd_auto').checked = upd.auto_check !== false;
      document.getElementById('upd_interval').value = String(upd.check_interval_hours || 24);
      document.getElementById('upd_repo').value = upd.repo || 'q871958549/Tomoyo';

      const ptb = document.getElementById('plugins-tbody');
      ptb.innerHTML = '';
      (INIT.plugins || []).forEach(p => {
        const tr = document.createElement('tr');
        tr.innerHTML = `<td><b>${esc(p.name)}</b><br><span style="color:var(--muted);font-size:11px">${esc(p.id)}</span></td>
          <td><span class="pill">${esc(p.runtime)}</span></td>
          <td>${esc((p.keywords || []).join(', '))}</td>
          <td>${p.max_memory_mb} MB</td>`;
        ptb.appendChild(tr);
      });
      document.getElementById('cfg-path').textContent = INIT.config_path || '';
      document.getElementById('plug-dir').textContent = INIT.plugins_dir || '';

      applyI18n();
    }

    function openConfigPath() {
      if (INIT.config_path) post({ cmd: 'open_path', path: INIT.config_path });
    }
    function openPluginsDir() {
      if (INIT.plugins_dir) post({ cmd: 'open_path', path: INIT.plugins_dir });
    }
    function closeWin() { post({ cmd: 'close' }); }

    populate();
