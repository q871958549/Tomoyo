    // --- Translation Engine Settings Controls (Hotkey Recorder, Engines, DPAPI Secrets) ---
    let recordingTrHotkey = false;
    let secretStatus = { deepl: false, openai: false };

    const TR_ENGINE_META = [
      { id: 'google', zh: 'Google 翻译（免费免密钥）', en: 'Google Translate (Free)' },
      { id: 'mymemory', zh: 'MyMemory + 有道词典（免费）', en: 'MyMemory + Youdao Dict (Free)' },
      { id: 'deepl', zh: 'DeepL API（需密钥）', en: 'DeepL API (Key)' },
      { id: 'openai', zh: 'OpenAI 兼容接口（需密钥）', en: 'OpenAI-Compatible (Key)' },
      { id: 'ollama', zh: '本地 Ollama 离线大模型', en: 'Local Ollama (Offline)' }
    ];

    function trState() {
      if (!state.translate) state.translate = {};
      if (!Array.isArray(state.translate.engines) || !state.translate.engines.length) {
        state.translate.engines = ['google', 'mymemory'];
      }
      return state.translate;
    }

    function renderTrHotkeyDisplay() {
      const el = document.getElementById('tr-hotkey-recorder');
      if (!el) return;
      const hk = (state.translate && state.translate.hotkey) || '';
      el.classList.remove('recording');
      if (!hk) {
        el.innerHTML = `<span style="color:var(--muted)">${esc(t('clip_hk_off'))}</span>`;
      } else {
        el.innerHTML = hk.split('+').map(p => `<kbd>${esc(p.trim())}</kbd>`).join(' + ');
      }
    }

    function startTrHotkeyRecording() {
      const el = document.getElementById('tr-hotkey-recorder');
      if (!el) return;
      recordingTrHotkey = true;
      el.classList.add('recording');
      el.textContent = t('rec_prompt');
      el.focus();
    }

    function stopTrHotkeyRecording() {
      recordingTrHotkey = false;
      renderTrHotkeyDisplay();
    }

    function applyTrHotkey(spec) {
      trState().hotkey = spec === '__off__' ? '' : spec;
      stopTrHotkeyRecording();
      scheduleAutoSave(0);
    }

    function renderTrEngineChips() {
      const box = document.getElementById('tr-engine-chips');
      if (!box) return;
      const active = new Set(trState().engines);
      const lang = curLang();
      box.innerHTML = '';
      TR_ENGINE_META.forEach(m => {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'filter-chip' + (active.has(m.id) ? ' active' : '');
        btn.textContent = (active.has(m.id) ? '✓ ' : '+ ') + (lang === 'en' ? m.en : m.zh);
        btn.onclick = () => {
          const list = trState().engines.slice();
          const idx = list.indexOf(m.id);
          if (idx >= 0) {
            if (list.length <= 1) return;
            list.splice(idx, 1);
          } else {
            list.push(m.id);
          }
          state.translate.engines = list;
          renderTrEngineChips();
          scheduleAutoSave(0);
        };
        box.appendChild(btn);
      });
    }

    function renderSecretBadges() {
      const dl = document.getElementById('tr-deepl-badge');
      const oa = document.getElementById('tr-openai-badge');
      const setTxt = t('tr_secret_saved');
      const noneTxt = t('tr_secret_none');
      if (dl) {
        dl.textContent = secretStatus.deepl ? setTxt : noneTxt;
        dl.style.color = secretStatus.deepl ? 'var(--ok)' : 'var(--muted)';
      }
      if (oa) {
        oa.textContent = secretStatus.openai ? setTxt : noneTxt;
        oa.style.color = secretStatus.openai ? 'var(--ok)' : 'var(--muted)';
      }
    }

    function saveApiSecret(keyName, inputId) {
      const inp = document.getElementById(inputId);
      if (!inp) return;
      const val = inp.value.trim();
      if (!val) {
        setToast(t('tr_secret_empty_warn'), true);
        return;
      }
      inp.value = '';
      post({ cmd: 'set_secret', key: keyName, value: val });
    }

    function clearApiSecret(keyName) {
      post({ cmd: 'delete_secret', key: keyName });
    }

    function onSecretsStatusEvent(data) {
      if (data && data.status) secretStatus = Object.assign(secretStatus, data.status);
      renderSecretBadges();
      if (data && data.message) setToast((data.ok ? '✓ ' : '✗ ') + data.message, !data.ok);
    }

    function syncTranslateControlsFromState() {
      const tr = trState();
      const tgt = document.getElementById('tr_target_lang');
      if (tgt) tgt.value = tr.target_lang || 'auto';
      const oaUrl = document.getElementById('tr_openai_url');
      if (oaUrl) oaUrl.value = tr.openai_base_url || 'https://api.openai.com/v1';
      const oaMod = document.getElementById('tr_openai_model');
      if (oaMod) oaMod.value = tr.openai_model || 'gpt-4o-mini';
      const olUrl = document.getElementById('tr_ollama_url');
      if (olUrl) olUrl.value = tr.ollama_url || 'http://127.0.0.1:11434';
      const olMod = document.getElementById('tr_ollama_model');
      if (olMod) olMod.value = tr.ollama_model || 'qwen2.5:7b';
      renderTrHotkeyDisplay();
      renderTrEngineChips();
      renderSecretBadges();
    }

    function mountTranslateSettingsControls() {
      secretStatus = Object.assign(
        { deepl: false, openai: false },
        (typeof INIT !== 'undefined' && INIT && INIT.secrets_status) || {}
      );
      const host = document.getElementById('translate-mount');
      if (!host) return;
      host.innerHTML = `
        <div style="display:flex;align-items:center;justify-content:space-between;gap:12px">
          <div><h2 data-i18n="h_translate"></h2><p class="lead" data-i18n="lead_translate"></p></div>
          <button class="btn primary" onclick="post({cmd:'open_translator',text:'',target_lang:''})" data-i18n="btn_open_tr"></button>
        </div>
        <div class="card">
          <div class="row">
            <div><div class="label-title" data-i18n="lbl_tr_hk"></div><div class="label-desc" data-i18n="desc_tr_hk"></div></div>
            <div class="hotkey-box">
              <div id="tr-hotkey-recorder" class="hotkey-recorder" tabindex="0"></div>
              <select id="tr-hotkey-preset" onchange="if(this.value){applyTrHotkey(this.value);this.value='';}">
                <option value="" data-i18n="preset_opt"></option>
                <option value="Alt+T">Alt+T</option>
                <option value="Alt+D">Alt+D</option>
                <option value="Ctrl+Shift+D">Ctrl+Shift+D</option>
                <option value="__off__" data-i18n="clip_hk_off"></option>
              </select>
            </div>
          </div>
          <div class="row">
            <div><div class="label-title" data-i18n="lbl_tr_tgt"></div><div class="label-desc" data-i18n="desc_tr_tgt"></div></div>
            <select id="tr_target_lang" onchange="scheduleAutoSave(0)">
              <option value="auto">Auto (中 ⇄ 英)</option>
              <option value="zh-Hans">简体中文</option>
              <option value="en">English</option>
              <option value="ja">日本語</option>
              <option value="ko">한국어</option>
              <option value="fr">Français</option>
              <option value="de">Deutsch</option>
            </select>
          </div>
          <div class="row" style="align-items:flex-start;flex-direction:column;gap:8px">
            <div><div class="label-title" data-i18n="lbl_tr_eng"></div><div class="label-desc" data-i18n="desc_tr_eng"></div></div>
            <div id="tr-engine-chips" class="chip-group"></div>
          </div>
        </div>
        <div class="card">
          <div class="row" style="align-items:flex-start;flex-direction:column;gap:8px">
            <div style="display:flex;width:100%;align-items:center;justify-content:space-between">
              <div class="label-title">DeepL API Key <span id="tr-deepl-badge" class="pill"></span></div>
              <button class="btn danger" onclick="clearApiSecret('deepl_api_key')" data-i18n="btn_clear_key"></button>
            </div>
            <div style="display:flex;width:100%;gap:8px">
              <input id="tr-deepl-key" type="password" autocomplete="new-password" style="flex:1" placeholder="输入 DeepL Free (:fx) 或 Pro Key，通过 Windows DPAPI 加密保存" />
              <button class="btn primary" onclick="saveApiSecret('deepl_api_key','tr-deepl-key')" data-i18n="btn_save_key"></button>
            </div>
          </div>
          <div class="row" style="align-items:flex-start;flex-direction:column;gap:8px">
            <div style="display:flex;width:100%;align-items:center;justify-content:space-between">
              <div class="label-title">OpenAI 兼容接口 Key <span id="tr-openai-badge" class="pill"></span></div>
              <button class="btn danger" onclick="clearApiSecret('openai_api_key')" data-i18n="btn_clear_key"></button>
            </div>
            <div style="display:flex;width:100%;gap:8px">
              <input id="tr-openai-key" type="password" autocomplete="new-password" style="flex:1" placeholder="输入 OpenAI / DeepSeek / 通义千问等兼容 API Key（加密写入）" />
              <button class="btn primary" onclick="saveApiSecret('openai_api_key','tr-openai-key')" data-i18n="btn_save_key"></button>
            </div>
            <div style="display:flex;width:100%;gap:8px;flex-wrap:wrap">
              <input id="tr_openai_url" type="url" style="flex:2;min-width:220px" placeholder="https://api.openai.com/v1" onchange="scheduleAutoSave(0)" />
              <input id="tr_openai_model" type="text" maxlength="64" style="flex:1;min-width:140px" placeholder="gpt-4o-mini" onchange="scheduleAutoSave(0)" />
            </div>
          </div>
          <div class="row" style="align-items:flex-start;flex-direction:column;gap:8px">
            <div class="label-title" data-i18n="lbl_tr_ollama"></div>
            <div style="display:flex;width:100%;gap:8px;flex-wrap:wrap">
              <input id="tr_ollama_url" type="url" style="flex:2;min-width:220px" placeholder="http://127.0.0.1:11434" onchange="scheduleAutoSave(0)" />
              <input id="tr_ollama_model" type="text" maxlength="64" style="flex:1;min-width:140px" placeholder="qwen2.5:7b" onchange="scheduleAutoSave(0)" />
            </div>
          </div>
        </div>
      `;
      const rec = document.getElementById('tr-hotkey-recorder');
      rec.addEventListener('click', startTrHotkeyRecording);
      rec.addEventListener('blur', () => { if (recordingTrHotkey) stopTrHotkeyRecording(); });
    }
