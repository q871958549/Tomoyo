    // --- Native Screenshot, Annotation, Color Picker & Pin-to-Desktop Controls ---
    let recordingCapHotkey = false;

    function capState() {
      if (!state.capture) state.capture = {};
      if (typeof state.capture.hotkey !== 'string') state.capture.hotkey = 'Alt+S';
      if (typeof state.capture.save_dir !== 'string') state.capture.save_dir = '';
      if (typeof state.capture.pin_opacity !== 'number') state.capture.pin_opacity = 100;
      if (typeof state.capture.copy_hex_lowercase !== 'boolean') state.capture.copy_hex_lowercase = false;
      return state.capture;
    }

    function renderCapHotkeyDisplay() {
      const el = document.getElementById('cap-hotkey-recorder');
      if (!el) return;
      const hk = capState().hotkey || '';
      el.classList.remove('recording');
      if (!hk) {
        el.innerHTML = `<span style="color:var(--muted)">${esc(t('clip_hk_off'))}</span>`;
      } else {
        el.innerHTML = hk.split('+').map(p => `<kbd>${esc(p.trim())}</kbd>`).join(' + ');
      }
    }

    function startCapHotkeyRecording() {
      const el = document.getElementById('cap-hotkey-recorder');
      if (!el) return;
      recordingCapHotkey = true;
      el.classList.add('recording');
      el.textContent = t('rec_prompt');
      el.focus();
    }

    function stopCapHotkeyRecording() {
      recordingCapHotkey = false;
      renderCapHotkeyDisplay();
    }

    function applyCapHotkey(spec) {
      capState().hotkey = spec === '__off__' ? '' : spec;
      stopCapHotkeyRecording();
      scheduleAutoSave(0);
    }

    function updateCapOpacityLabel(v) {
      const el = document.getElementById('cap_opacity-v');
      if (el) el.textContent = String(v) + '%';
    }

    function renderCapSaveDir() {
      const el = document.getElementById('cap-save-dir-val');
      const clr = document.getElementById('btn-cap-dir-clear');
      const dir = (capState().save_dir || '').trim();
      if (el) el.textContent = dir || t('cap_dir_default');
      if (clr) clr.style.display = dir ? '' : 'none';
    }

    function pickCaptureDir() {
      post({ cmd: 'pick_capture_dir' });
    }

    function clearCaptureDir() {
      capState().save_dir = '';
      renderCapSaveDir();
      scheduleAutoSave(0);
    }

    function onCaptureDirPicked(data) {
      if (!data || data.cancelled) return;
      if (data.ok && data.path) {
        capState().save_dir = data.path;
        renderCapSaveDir();
        scheduleAutoSave(0);
      } else if (data.error) {
        setToast('✗ ' + data.error, true);
      }
    }

    function triggerCaptureFromSettings(mode) {
      post({ cmd: 'start_capture', mode: mode || 'region' });
    }

    function syncCaptureControlsFromState() {
      const cap = capState();
      const op = Math.min(100, Math.max(30, Number(cap.pin_opacity || 100)));
      const opInp = document.getElementById('cap_opacity');
      if (opInp) opInp.value = String(op);
      updateCapOpacityLabel(op);
      const hexChk = document.getElementById('cap_hex_lower');
      if (hexChk) hexChk.checked = !!cap.copy_hex_lowercase;
      renderCapHotkeyDisplay();
      renderCapSaveDir();
    }

    function mountCaptureSettingsControls() {
      const host = document.getElementById('capture-mount');
      if (!host) return;
      host.innerHTML = `
        <div style="display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap">
          <div><h2 data-i18n="h_capture"></h2><p class="lead" data-i18n="lead_capture"></p></div>
          <div style="display:flex;gap:8px;flex-wrap:wrap">
            <button class="btn primary" onclick="triggerCaptureFromSettings('region')" data-i18n="btn_cap_region"></button>
            <button class="btn" onclick="triggerCaptureFromSettings('ocr')" data-i18n="btn_cap_ocr"></button>
            <button class="btn" onclick="triggerCaptureFromSettings('translate')" data-i18n="btn_cap_tr"></button>
            <button class="btn" onclick="triggerCaptureFromSettings('pin_clipboard')" data-i18n="btn_cap_pin"></button>
          </div>
        </div>
        <div class="card">
          <div class="row">
            <div><div class="label-title" data-i18n="lbl_cap_hk"></div><div class="label-desc" data-i18n="desc_cap_hk"></div></div>
            <div class="hotkey-box">
              <div id="cap-hotkey-recorder" class="hotkey-recorder" tabindex="0"></div>
              <select id="cap-hotkey-preset" onchange="if(this.value){applyCapHotkey(this.value);this.value='';}">
                <option value="" data-i18n="preset_opt"></option>
                <option value="Alt+S">Alt+S</option>
                <option value="Alt+A">Alt+A</option>
                <option value="Ctrl+Shift+A">Ctrl+Shift+A</option>
                <option value="Alt+Q">Alt+Q</option>
                <option value="__off__" data-i18n="clip_hk_off"></option>
              </select>
            </div>
          </div>
          <div class="row">
            <div><div class="label-title" data-i18n="lbl_cap_opacity"></div><div class="label-desc" data-i18n="desc_cap_opacity"></div></div>
            <div class="range-wrap">
              <input id="cap_opacity" type="range" min="30" max="100" step="5"
                     oninput="updateCapOpacityLabel(this.value);scheduleAutoSave(160)" />
              <span id="cap_opacity-v" class="range-val">100%</span>
            </div>
          </div>
          <div class="row">
            <div><div class="label-title" data-i18n="lbl_cap_hex"></div><div class="label-desc" data-i18n="desc_cap_hex"></div></div>
            <input id="cap_hex_lower" type="checkbox" onchange="scheduleAutoSave(0)" />
          </div>
          <div class="row">
            <div style="min-width:0;flex:1">
              <div class="label-title" data-i18n="lbl_cap_dir"></div>
              <div id="cap-save-dir-val" class="label-desc" style="word-break:break-all"></div>
            </div>
            <div style="display:flex;gap:8px;flex-shrink:0">
              <button class="btn" id="btn-cap-dir-clear" onclick="clearCaptureDir()" data-i18n="btn_cap_dir_reset"></button>
              <button class="btn primary" onclick="pickCaptureDir()" data-i18n="btn_browse_dir"></button>
            </div>
          </div>
        </div>
        <div class="card">
          <div class="label-title" data-i18n="lbl_cap_tips"></div>
          <div class="label-desc" style="margin-top:6px;line-height:1.7" data-i18n="desc_cap_tips"></div>
        </div>
      `;
      const rec = document.getElementById('cap-hotkey-recorder');
      rec.addEventListener('click', startCapHotkeyRecording);
      rec.addEventListener('blur', () => { if (recordingCapHotkey) stopCapHotkeyRecording(); });
    }
