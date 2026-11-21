    // Sidebar navigation (INIT, state, autostart, t, esc, post, setToast come from settings_boot.js)
    let storeLoadedOnce = false;
    let updatesCheckedInSession = false;
    document.querySelectorAll('nav button').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('nav button').forEach(b => b.classList.remove('active'));
        document.querySelectorAll('section').forEach(s => s.classList.remove('active'));
        btn.classList.add('active');
        document.getElementById('tab-' + btn.dataset.tab).classList.add('active');
        if (btn.dataset.tab === 'store' && !storeLoadedOnce) {
          storeLoadedOnce = true;
          fetchStore();
        } else if (btn.dataset.tab === 'logs') {
          refreshLogs();
        } else if (btn.dataset.tab === 'updates' && !updatesCheckedInSession) {
          updatesCheckedInSession = true;
          const nowSec = Math.floor(Date.now() / 1000);
          const lastSec = (INIT.cached_update && INIT.cached_update.checked_at_unix) || 0;
          if (!INIT.cached_update || (nowSec - lastSec) > 300) {
            checkUpdatesNow();
          }
        }
      });
    });

    function applyI18n() {
      const lang = curLang();
      document.documentElement.lang = lang === 'en' ? 'en' : 'zh-CN';
      document.getElementById('lang-zh').classList.toggle('active', lang === 'zh');
      document.getElementById('lang-en').classList.toggle('active', lang === 'en');
      document.querySelectorAll('[data-i18n]').forEach(el => {
        const val = t(el.getAttribute('data-i18n'));
        if (typeof val === 'string') el.textContent = val;
      });
      updateRowsLabel(document.getElementById('win_rows').value);
      updateEvGlobalLabel(document.getElementById('ev_global').value);
      updateEvDedLabel(document.getElementById('ev_ded').value);
      renderAppsDepth();
      renderDirsList();
      renderEvFilterChips();
      renderClipExclChips();
      const exclInp = document.getElementById('clip-excl-search');
      if (exclInp) exclInp.placeholder = t('clip_excl_ph');
      updatePosDesc();
      renderWebRows();
      if (!recordingHotkey) renderHotkeyDisplay();
      if (!recordingClipHotkey) renderClipHotkeyDisplay();
      if (!recordingTrHotkey) renderTrHotkeyDisplay();
      if (!recordingCapHotkey) renderCapHotkeyDisplay();
      renderCapSaveDir();
      renderTrEngineChips();
      renderSecretBadges();
      setToast(t('toast_ready'), false);
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

    function updateRowsLabel(v) { document.getElementById('win_rows-v').textContent = t('rows_unit')(v); }
    function updateEvGlobalLabel(v) { document.getElementById('ev_global-v').textContent = t('items_unit')(v); }
    function updateEvDedLabel(v) { document.getElementById('ev_ded-v').textContent = t('items_unit')(v); }

    function updatePosDesc() {
      const w = state.window || {};
      const el = document.getElementById('pos-desc');
      el.textContent = (w.custom_x != null && w.custom_y != null)
        ? t('pos_saved')(w.custom_x, w.custom_y)
        : t('pos_auto');
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
    function stopHotkeyRecording() { recordingHotkey = false; renderHotkeyDisplay(); }
    function applyHotkey(spec) { state.hotkey = spec; stopHotkeyRecording(); scheduleAutoSave(0); }
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
      if (recordingHotkey || recordingClipHotkey || recordingTrHotkey || recordingCapHotkey) {
        e.preventDefault();
        e.stopPropagation();
        if (e.key === 'Escape') {
          if (recordingHotkey) stopHotkeyRecording();
          if (recordingClipHotkey) stopClipHotkeyRecording();
          if (recordingTrHotkey) stopTrHotkeyRecording();
          if (recordingCapHotkey) stopCapHotkeyRecording();
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
          const target = recordingHotkey
            ? recEl
            : recordingClipHotkey
              ? document.getElementById('clip-hotkey-recorder')
              : recordingTrHotkey
                ? document.getElementById('tr-hotkey-recorder')
                : document.getElementById('cap-hotkey-recorder');
          if (target) target.textContent = t('rec_need_mod');
          return;
        }
        const combo = mods.concat([mainKey]).join('+');
        if (recordingHotkey) applyHotkey(combo);
        else if (recordingClipHotkey) applyClipHotkey(combo);
        else if (recordingTrHotkey) applyTrHotkey(combo);
        else applyCapHotkey(combo);
        return;
      }
      if (e.key === 'Escape') closeWin();
    }, true);

    function collectAndSave() {
      if (!validateAllWebRows()) return;
      const repoMode = document.getElementById('upd_repo_mode').value;
      const customRepo = document.getElementById('upd_repo').value.trim();
      if (repoMode === 'custom' && !isValidRepo(customRepo)) return;

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
      const prevApps = state.apps || {};
      state.apps = {
        extra_dirs: Array.isArray(prevApps.extra_dirs) ? prevApps.extra_dirs : [],
        auto_detect_desktop_organizers: document.getElementById('apps_org').checked,
        extra_scan_depth: Math.min(4, Math.max(1, Number(prevApps.extra_scan_depth) || 2))
      };
      const prevEv = state.everything || {};
      const evGlobalVal = parseInt(document.getElementById('ev_global').value, 10);
      state.everything = {
        max_global_results: Number.isNaN(evGlobalVal) ? 6 : evGlobalVal,
        max_dedicated_results: parseInt(document.getElementById('ev_ded').value, 10) || 30,
        extra_filter: (prevEv.extra_filter || '').trim(),
        favorites: prevEv.favorites
      };
      const prevCb = state.clipboard || {};
      state.clipboard = {
        persist_sqlite: document.getElementById('clip_persist').checked,
        max_persisted_items: snapClipMax(document.getElementById('clip_max').value),
        hotkey: String(prevCb.hotkey || '').trim(),
        capture_enabled: document.getElementById('clip_capture') ? document.getElementById('clip_capture').checked : true,
        capture_images: document.getElementById('clip_images') ? document.getElementById('clip_images').checked : true,
        excluded_apps: Array.isArray(prevCb.excluded_apps) ? prevCb.excluded_apps : []
      };
      const prevTr = state.translate || {};
      state.translate = {
        hotkey: String(prevTr.hotkey || '').trim(),
        target_lang: document.getElementById('tr_target_lang') ? document.getElementById('tr_target_lang').value : 'auto',
        engines: Array.isArray(prevTr.engines) && prevTr.engines.length ? prevTr.engines : ['google', 'mymemory'],
        openai_base_url: document.getElementById('tr_openai_url') ? document.getElementById('tr_openai_url').value.trim() : 'https://api.openai.com/v1',
        openai_model: document.getElementById('tr_openai_model') ? document.getElementById('tr_openai_model').value.trim() : 'gpt-4o-mini',
        ollama_url: document.getElementById('tr_ollama_url') ? document.getElementById('tr_ollama_url').value.trim() : 'http://127.0.0.1:11434',
        ollama_model: document.getElementById('tr_ollama_model') ? document.getElementById('tr_ollama_model').value.trim() : 'qwen2.5:7b'
      };
      const prevCap = state.capture || {};
      state.capture = {
        hotkey: String(prevCap.hotkey || '').trim(),
        save_dir: String(prevCap.save_dir || '').trim(),
        pin_opacity: Math.min(100, Math.max(30, parseInt((document.getElementById('cap_opacity') && document.getElementById('cap_opacity').value) || '100', 10) || 100)),
        copy_hex_lowercase: !!(document.getElementById('cap_hex_lower') && document.getElementById('cap_hex_lower').checked)
      };
      const prevUpd = state.update || {};
      state.update = {
        auto_check: document.getElementById('upd_auto').checked,
        check_interval_hours: parseInt(document.getElementById('upd_interval').value, 10) || 24,
        repo: repoMode === 'custom' ? customRepo : OFFICIAL_REPO,
        last_checked_unix: prevUpd.last_checked_unix || 0,
        skipped_version: prevUpd.skipped_version || null
      };
      post({ cmd: 'save_settings', config: state, autostart });
      setToast(t('toast_saved') + ' · ' + new Date().toLocaleTimeString(), false);
    }

    function onSettingsSavedAck(data) {
      if (!data) return;
      if (data.config) state = data.config;
      if (Array.isArray(data.notes) && data.notes.length > 0) {
        renderAppsDepth();
        renderDirsList();
        renderEvFilterChips();
        renderClipHotkeyDisplay();
        renderClipExclChips();
        syncTranslateControlsFromState();
        syncCaptureControlsFromState();
        syncRepoUi();
        setToast('⚠ ' + data.notes[0], true);
      } else {
        setToast(t('toast_saved') + ' · ' + new Date().toLocaleTimeString(), false);
      }
    }

    function scheduleAutoSave(delayMs) {
      if (saveTimer) clearTimeout(saveTimer);
      if (delayMs <= 0) collectAndSave();
      else saveTimer = setTimeout(collectAndSave, delayMs);
    }

    function populate() {
      mountClipSettingsControls();
      mountTranslateSettingsControls();
      mountCaptureSettingsControls();
      if (!state.language) state.language = 'zh';
      document.getElementById('about-ver').textContent = 'v' + (INIT.version || '1.5.0');
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
      if (!state.apps) state.apps = {};
      state.apps.extra_scan_depth = Math.min(4, Math.max(1, Number(appsCfg.extra_scan_depth ?? 2)));
      state.apps.extra_dirs = Array.isArray(appsCfg.extra_dirs) ? appsCfg.extra_dirs : [];

      const ev = state.everything || {};
      document.getElementById('ev_global').value = ev.max_global_results ?? 6;
      document.getElementById('ev_ded').value = Math.min(50, Math.max(10, Number(ev.max_dedicated_results ?? 30)));

      const cb = state.clipboard || {};
      document.getElementById('clip_persist').checked = cb.persist_sqlite !== false;
      document.getElementById('clip_max').value = String(snapClipMax(cb.max_persisted_items ?? 500));
      if (document.getElementById('clip_capture')) document.getElementById('clip_capture').checked = cb.capture_enabled !== false;
      if (document.getElementById('clip_images')) document.getElementById('clip_images').checked = cb.capture_images !== false;
      if (!state.clipboard) state.clipboard = {};
      state.clipboard.hotkey = typeof cb.hotkey === 'string' ? cb.hotkey : 'Alt+V';
      state.clipboard.excluded_apps = Array.isArray(cb.excluded_apps) ? cb.excluded_apps : [];
      if (!state.translate) state.translate = { hotkey: 'Alt+T', target_lang: 'auto', engines: ['google', 'mymemory'] };
      syncTranslateControlsFromState();
      if (!state.capture) state.capture = { hotkey: 'Alt+S', save_dir: '', pin_opacity: 100, copy_hex_lowercase: false };
      syncCaptureControlsFromState();

      const upd = state.update || {};
      document.getElementById('upd_auto').checked = upd.auto_check !== false;
      document.getElementById('upd_interval').value = String(upd.check_interval_hours || 24);
      syncRepoUi();

      const ptb = document.getElementById('plugins-tbody');
      ptb.innerHTML = '';
      (INIT.plugins || []).forEach(p => {
        const declared = p.declared_permissions || [];
        const granted = new Set(p.granted_permissions || []);
        const permHtml = declared.length
          ? declared.map(perm => {
              const checked = granted.has(perm) ? 'checked' : '';
              return `<label style="display:inline-flex;align-items:center;gap:4px;margin-right:8px;font-size:11.5px;cursor:pointer">
                <input type="checkbox" ${checked} onchange="togglePluginPermission(${JSON.stringify(p.id)},${JSON.stringify(perm)},this.checked)" />
                <span>${esc(perm)}</span>
              </label>`;
            }).join('')
          : `<span style="color:var(--muted);font-size:11.5px">${esc(t('perm_none'))}</span>`;
        const tr = document.createElement('tr');
        tr.innerHTML = `<td><b>${esc(p.name)}</b><br><span style="color:var(--muted);font-size:11px">${esc(p.id)}</span></td>
          <td><span class="pill">${esc(p.runtime)}</span></td>
          <td>${esc((p.keywords || []).join(', '))}</td>
          <td>${permHtml}</td>
          <td>${p.max_memory_mb} MB</td>`;
        ptb.appendChild(tr);
      });
      document.getElementById('cfg-path').textContent = INIT.config_path || '';
      document.getElementById('plug-dir').textContent = INIT.plugins_dir || '';
      if (INIT.cached_update && typeof renderUpdateStatus === 'function') {
        renderUpdateStatus(INIT.cached_update);
      }

      applyI18n();
    }

    function togglePluginPermission(pluginId, permission, allowed) {
      post({ cmd: 'set_plugin_permission', plugin_id: pluginId, permission, allowed });
    }
    function openConfigPath() { if (INIT.config_path) post({ cmd: 'open_path', path: INIT.config_path }); }
    function openPluginsDir() { if (INIT.plugins_dir) post({ cmd: 'open_path', path: INIT.plugins_dir }); }
    function closeWin() { post({ cmd: 'close' }); }

    populate();
