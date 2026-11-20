    // --- Clipboard Dedicated Hotkey Recorder & Privacy App Searchable Combobox ---
    let recordingClipHotkey = false;

    function clipExclApps() {
      if (!state.clipboard) state.clipboard = {};
      if (!Array.isArray(state.clipboard.excluded_apps)) state.clipboard.excluded_apps = [];
      return state.clipboard.excluded_apps;
    }

    function renderClipHotkeyDisplay() {
      const el = document.getElementById('clip-hotkey-recorder');
      if (!el) return;
      const hk = (state.clipboard && state.clipboard.hotkey) || '';
      el.classList.remove('recording');
      if (!hk) {
        el.innerHTML = `<span style="color:var(--muted)">${esc(t('clip_hk_off'))}</span>`;
      } else {
        el.innerHTML = hk.split('+').map(p => `<kbd>${esc(p.trim())}</kbd>`).join(' + ');
      }
    }

    function startClipHotkeyRecording() {
      const el = document.getElementById('clip-hotkey-recorder');
      if (!el) return;
      recordingClipHotkey = true;
      el.classList.add('recording');
      el.textContent = t('rec_prompt');
      el.focus();
    }

    function stopClipHotkeyRecording() {
      recordingClipHotkey = false;
      renderClipHotkeyDisplay();
    }

    function applyClipHotkey(spec) {
      if (!state.clipboard) state.clipboard = {};
      state.clipboard.hotkey = spec === '__off__' ? '' : spec;
      stopClipHotkeyRecording();
      scheduleAutoSave(0);
    }

    function normalizeExeInput(raw) {
      let s = String(raw || '').trim().toLowerCase();
      if (!s || /[\\/]|\.\./.test(s) || s.length > 64) return null;
      if (!s.endsWith('.exe')) s += '.exe';
      const stem = s.slice(0, -4);
      if (!stem || !/^[a-z0-9._ -]+$/.test(stem)) return null;
      return s;
    }

    function renderClipExclChips() {
      const box = document.getElementById('clip-excl-chips');
      if (!box) return;
      const apps = clipExclApps();
      box.innerHTML = '';
      if (!apps.length) {
        box.innerHTML = `<span class="label-desc">${esc(t('clip_excl_empty'))}</span>`;
        return;
      }
      apps.forEach((exe, idx) => {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'filter-chip active';
        btn.textContent = `${exe} ×`;
        btn.onclick = () => {
          apps.splice(idx, 1);
          renderClipExclChips();
          scheduleAutoSave(0);
        };
        box.appendChild(btn);
      });
    }

    function addClipExclApp(raw) {
      const exe = normalizeExeInput(raw);
      const err = document.getElementById('clip-excl-err');
      if (!exe) {
        if (err) { err.style.display = 'block'; err.textContent = t('clip_excl_invalid'); }
        return;
      }
      if (err) err.style.display = 'none';
      const apps = clipExclApps();
      if (!apps.includes(exe) && apps.length < 32) {
        apps.push(exe);
        renderClipExclChips();
        scheduleAutoSave(0);
      }
      const inp = document.getElementById('clip-excl-search');
      if (inp) inp.value = '';
      closeClipCombo();
    }

    function renderClipComboOptions() {
      const inp = document.getElementById('clip-excl-search');
      const menu = document.getElementById('clip-excl-menu');
      if (!inp || !menu) return;
      const q = inp.value.trim().toLowerCase();
      const existing = new Set(clipExclApps().map(s => s.toLowerCase()));
      const pool = (INIT.running_apps || []).filter(exe => !existing.has(exe.toLowerCase()) && (!q || exe.toLowerCase().includes(q)));
      menu.innerHTML = '';
      pool.slice(0, 10).forEach(exe => {
        const opt = document.createElement('button');
        opt.type = 'button';
        opt.className = 'combo-opt';
        opt.textContent = exe;
        opt.onmousedown = (e) => { e.preventDefault(); addClipExclApp(exe); };
        menu.appendChild(opt);
      });
      const customExe = q ? normalizeExeInput(q) : null;
      if (customExe && !existing.has(customExe) && !pool.includes(customExe)) {
        const opt = document.createElement('button');
        opt.type = 'button';
        opt.className = 'combo-opt';
        opt.textContent = `+ ${customExe}`;
        opt.onmousedown = (e) => { e.preventDefault(); addClipExclApp(customExe); };
        menu.appendChild(opt);
      }
      menu.style.display = menu.children.length ? 'flex' : 'none';
    }

    function closeClipCombo() {
      const menu = document.getElementById('clip-excl-menu');
      if (menu) menu.style.display = 'none';
    }

    function mountClipSettingsControls() {
      const host = document.getElementById('clip-advanced-mount');
      if (!host) return;
      host.innerHTML = `
        <div class="row">
          <div><div class="label-title" data-i18n="lbl_clip_hk"></div><div class="label-desc" data-i18n="desc_clip_hk"></div></div>
          <div class="hotkey-box">
            <div id="clip-hotkey-recorder" class="hotkey-recorder" tabindex="0"></div>
            <select id="clip-hotkey-preset" onchange="if(this.value){applyClipHotkey(this.value);this.value='';}">
              <option value="" data-i18n="preset_opt"></option>
              <option value="Alt+V">Alt+V</option>
              <option value="Ctrl+Shift+V">Ctrl+Shift+V</option>
              <option value="Alt+C">Alt+C</option>
              <option value="__off__" data-i18n="clip_hk_off"></option>
            </select>
          </div>
        </div>
        <div class="row">
          <div><div class="label-title" data-i18n="lbl_clip_cap"></div><div class="label-desc" data-i18n="desc_clip_cap"></div></div>
          <div style="display:flex;align-items:center;gap:10px">
            <button class="btn" onclick="post({cmd:'open_clipboard_manager'})" data-i18n="btn_open_clip_mgr"></button>
            <input id="clip_capture" type="checkbox" onchange="scheduleAutoSave(0)" />
          </div>
        </div>
        <div class="row">
          <div><div class="label-title" data-i18n="lbl_clip_img"></div><div class="label-desc" data-i18n="desc_clip_img"></div></div>
          <input id="clip_images" type="checkbox" onchange="scheduleAutoSave(0)" />
        </div>
        <div class="row" style="align-items:flex-start;flex-direction:column;gap:8px">
          <div><div class="label-title" data-i18n="lbl_clip_excl"></div><div class="label-desc" data-i18n="desc_clip_excl"></div></div>
          <div class="combo-wrap">
            <input id="clip-excl-search" type="text" maxlength="48" autocomplete="off"
                   onfocus="renderClipComboOptions()" oninput="renderClipComboOptions()" onblur="setTimeout(closeClipCombo,120)"
                   onkeydown="if(event.key==='Enter'){event.preventDefault();addClipExclApp(this.value);}" />
            <div id="clip-excl-menu" class="combo-menu"></div>
          </div>
          <div id="clip-excl-chips" class="chip-group"></div>
          <div id="clip-excl-err" class="field-err" style="display:none"></div>
        </div>
      `;
      const rec = document.getElementById('clip-hotkey-recorder');
      rec.addEventListener('click', startClipHotkeyRecording);
      rec.addEventListener('blur', () => { if (recordingClipHotkey) stopClipHotkeyRecording(); });
    }
