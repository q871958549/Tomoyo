    // --- Bounded Controls: Scan Depth, Folder Picker, Filter Chips, Validated Web & Repo ---
    const OFFICIAL_REPO = 'q871958549/Tomoyo';
    const RESERVED_WEB_KW = new Set(['f', 'ev', 'cb', 'w', 'kill', 'ps', 'snip', '=', 'b64', 'b64d']);
    const CLIP_OPTIONS = [100, 200, 500, 1000, 2000];
    const EV_FILTER_PRESETS = [
      { tok: '!*.tmp', zh: '排除临时文件 (.tmp)', en: 'Exclude .tmp' },
      { tok: '!node_modules', zh: '排除 node_modules', en: 'Exclude node_modules' },
      { tok: '!.git', zh: '排除 .git 目录', en: 'Exclude .git' },
      { tok: '!*.log', zh: '排除日志 (.log)', en: 'Exclude .log' },
      { tok: '!target', zh: '排除 Rust target', en: 'Exclude target' },
      { tok: '!dist', zh: '排除构建目录 dist', en: 'Exclude dist' },
      { tok: '!*.bak', zh: '排除备份 (.bak)', en: 'Exclude .bak' }
    ];
    const WEB_PRESETS = {
      bing: { keyword: 'bing', name: 'Bing', url: 'https://www.bing.com/search?q={q}' },
      wiki: { keyword: 'wiki', name: 'Wikipedia', url: 'https://zh.wikipedia.org/wiki/Special:Search?search={q}' },
      bili: { keyword: 'bili', name: 'Bilibili', url: 'https://search.bilibili.com/all?keyword={q}' },
      npm: { keyword: 'npm', name: 'npm', url: 'https://www.npmjs.com/search?q={q}' },
      pypi: { keyword: 'pypi', name: 'PyPI', url: 'https://pypi.org/search/?q={q}' },
      so: { keyword: 'so', name: 'StackOverflow', url: 'https://stackoverflow.com/search?q={q}' },
      custom: { keyword: 'site', name: 'Custom', url: 'https://example.com/search?q={q}' }
    };

    function renderAppsDepth() {
      const cur = Math.min(4, Math.max(1, Number((state.apps && state.apps.extra_scan_depth) ?? 2)));
      document.querySelectorAll('#apps_depth_seg button[data-depth]').forEach(btn => {
        const d = Number(btn.dataset.depth);
        btn.textContent = t('depth_unit')(d);
        btn.classList.toggle('active', d === cur);
      });
    }
    function setAppsDepth(d) {
      if (!state.apps) state.apps = {};
      state.apps.extra_scan_depth = Math.min(4, Math.max(1, Number(d) || 2));
      renderAppsDepth();
      scheduleAutoSave(0);
    }

    function renderDirsList() {
      const box = document.getElementById('apps_dirs');
      const dirs = (state.apps && state.apps.extra_dirs) || [];
      box.innerHTML = '';
      if (!dirs.length) {
        box.innerHTML = `<div class="dir-empty">${esc(t('dirs_empty'))}</div>`;
        return;
      }
      dirs.forEach((dir, idx) => {
        const row = document.createElement('div');
        row.className = 'dir-row';
        row.innerHTML = `<span class="dir-path">${esc(dir)}</span><button class="btn danger" style="padding:3px 9px;font-size:12px" onclick="removeScanDir(${idx})">${esc(t('del'))}</button>`;
        box.appendChild(row);
      });
    }
    function showDirErr(msg) {
      const el = document.getElementById('apps_dirs_err');
      el.style.display = msg ? 'block' : 'none';
      el.textContent = msg || '';
    }
    function pickScanFolder() { showDirErr(''); post({ cmd: 'pick_folder' }); }
    function onFolderPicked(data) {
      if (!data || data.cancelled) return;
      if (!data.ok) { showDirErr(data.error || '目录不可用'); return; }
      if (!state.apps) state.apps = {};
      if (!Array.isArray(state.apps.extra_dirs)) state.apps.extra_dirs = [];
      const norm = String(data.path || '').trim();
      if (!norm) return;
      if (state.apps.extra_dirs.some(d => d.toLowerCase() === norm.toLowerCase())) {
        showDirErr(curLang() === 'en' ? 'Directory already added.' : '该目录已在扫描列表中。');
        return;
      }
      if (state.apps.extra_dirs.length >= 16) {
        showDirErr(curLang() === 'en' ? 'Maximum 16 custom directories reached.' : '最多允许添加 16 个自定义扫描目录。');
        return;
      }
      showDirErr('');
      state.apps.extra_dirs.push(norm);
      renderDirsList();
      scheduleAutoSave(0);
    }
    function removeScanDir(idx) {
      if (!state.apps || !Array.isArray(state.apps.extra_dirs)) return;
      state.apps.extra_dirs.splice(idx, 1);
      showDirErr('');
      renderDirsList();
      scheduleAutoSave(0);
    }

    function getEvFilterTokens() {
      const raw = (state.everything && state.everything.extra_filter) || '';
      return raw.split(/\s+/).map(s => s.trim()).filter(Boolean);
    }
    function renderEvFilterChips() {
      const wrap = document.getElementById('ev_filter_chips');
      const prev = document.getElementById('ev_filter_preview');
      const activeSet = new Set(getEvFilterTokens().map(s => s.toLowerCase()));
      const lang = curLang();
      wrap.innerHTML = '';
      EV_FILTER_PRESETS.forEach(p => {
        const btn = document.createElement('button');
        btn.type = 'button';
        const on = activeSet.has(p.tok.toLowerCase());
        btn.className = 'filter-chip' + (on ? ' active' : '');
        btn.textContent = (on ? '✓ ' : '+ ') + (lang === 'en' ? p.en : p.zh);
        btn.onclick = () => toggleEvFilterToken(p.tok);
        wrap.appendChild(btn);
      });
      const expr = getEvFilterTokens().join(' ');
      prev.textContent = expr ? t('ev_filter_cur')(expr) : t('ev_filter_none');
    }
    function toggleEvFilterToken(tok) {
      if (!state.everything) state.everything = {};
      const tokens = getEvFilterTokens();
      const low = tok.toLowerCase();
      const idx = tokens.findIndex(s => s.toLowerCase() === low);
      if (idx >= 0) tokens.splice(idx, 1);
      else tokens.push(tok);
      state.everything.extra_filter = tokens.join(' ');
      renderEvFilterChips();
      scheduleAutoSave(0);
    }

    function validateWebRow(w, allRows, idx) {
      const kw = String(w.keyword || '').trim().toLowerCase();
      const dup = allRows.some((o, j) => j !== idx && String(o.keyword || '').trim().toLowerCase() === kw);
      const kwOk = /^[a-z0-9_-]{1,16}$/.test(kw) && !RESERVED_WEB_KW.has(kw) && !dup;
      const name = String(w.name || '').trim();
      const nameOk = name.length >= 1 && name.length <= 40;
      const url = String(w.url || '').trim();
      const urlOk = /^https?:\/\/\S+$/i.test(url) && url.length <= 512 && url.includes('{q}');
      return { kwOk, nameOk, urlOk, ok: kwOk && nameOk && urlOk };
    }
    function validateAllWebRows() {
      const rows = state.web_searches || [];
      let allOk = true;
      const trs = document.querySelectorAll('#web-tbody tr');
      rows.forEach((w, idx) => {
        const v = validateWebRow(w, rows, idx);
        if (!v.ok) allOk = false;
        const tr = trs[idx];
        if (tr) {
          const inputs = tr.querySelectorAll('input');
          if (inputs[0]) inputs[0].classList.toggle('input-invalid', !v.kwOk);
          if (inputs[1]) inputs[1].classList.toggle('input-invalid', !v.nameOk);
          if (inputs[2]) inputs[2].classList.toggle('input-invalid', !v.urlOk);
        }
      });
      const err = document.getElementById('web-err');
      err.style.display = allOk ? 'none' : 'block';
      err.textContent = allOk ? '' : t('web_invalid');
      return allOk;
    }
    function renderWebRows() {
      const tb = document.getElementById('web-tbody');
      tb.innerHTML = '';
      (state.web_searches || []).forEach((w, idx) => {
        const tr = document.createElement('tr');
        tr.innerHTML = `
          <td><input type="text" maxlength="16" value="${esc(w.keyword)}" style="width:76px" oninput="onWebFieldInput(${idx},'keyword',this)" /></td>
          <td><input type="text" maxlength="40" value="${esc(w.name)}" style="width:128px" oninput="onWebFieldInput(${idx},'name',this)" /></td>
          <td><input type="text" maxlength="512" value="${esc(w.url)}" style="width:100%" oninput="onWebFieldInput(${idx},'url',this)" /></td>
          <td><button class="btn danger" style="padding:4px 8px" onclick="delWebRow(${idx})">${esc(t('del'))}</button></td>
        `;
        tb.appendChild(tr);
      });
      validateAllWebRows();
    }
    function onWebFieldInput(idx, field, el) {
      if (!state.web_searches || !state.web_searches[idx]) return;
      state.web_searches[idx][field] = el.value;
      if (validateAllWebRows()) scheduleAutoSave(350);
    }
    function addWebPreset(key) {
      if (!key || !WEB_PRESETS[key]) return;
      if (!state.web_searches) state.web_searches = [];
      const base = Object.assign({}, WEB_PRESETS[key]);
      const used = new Set(state.web_searches.map(w => String(w.keyword || '').trim().toLowerCase()));
      let kw = base.keyword;
      let n = 2;
      while (used.has(kw) || RESERVED_WEB_KW.has(kw)) { kw = base.keyword + n; n++; }
      base.keyword = kw;
      state.web_searches.push(base);
      renderWebRows();
      if (validateAllWebRows()) scheduleAutoSave(0);
    }
    function delWebRow(i) {
      state.web_searches.splice(i, 1);
      renderWebRows();
      if (validateAllWebRows()) scheduleAutoSave(0);
    }

    function isValidRepo(r) {
      return /^[A-Za-z0-9_.-]{1,64}\/[A-Za-z0-9_.-]{1,100}$/.test(r) && !r.includes('..');
    }
    function syncRepoUi() {
      const repo = (state.update && state.update.repo) || OFFICIAL_REPO;
      const isCustom = repo !== OFFICIAL_REPO;
      document.getElementById('upd_repo_mode').value = isCustom ? 'custom' : 'official';
      document.getElementById('upd_repo_custom_wrap').style.display = isCustom ? 'block' : 'none';
      const inp = document.getElementById('upd_repo');
      inp.value = repo;
      inp.classList.remove('input-invalid');
      document.getElementById('upd_repo_err').style.display = 'none';
    }
    function onRepoModeChanged(mode) {
      if (!state.update) state.update = {};
      if (mode === 'official') {
        state.update.repo = OFFICIAL_REPO;
        syncRepoUi();
        scheduleAutoSave(0);
      } else {
        document.getElementById('upd_repo_custom_wrap').style.display = 'block';
        const inp = document.getElementById('upd_repo');
        inp.focus();
        inp.select();
      }
    }
    function onCustomRepoInput() {
      const inp = document.getElementById('upd_repo');
      const err = document.getElementById('upd_repo_err');
      const val = inp.value.trim();
      const ok = isValidRepo(val);
      inp.classList.toggle('input-invalid', !ok);
      err.style.display = ok ? 'none' : 'block';
      err.textContent = ok ? '' : t('repo_invalid');
      if (ok) {
        if (!state.update) state.update = {};
        state.update.repo = val;
        scheduleAutoSave(350);
      }
    }
    function snapClipMax(val) {
      const n = Number(val) || 500;
      return CLIP_OPTIONS.reduce((best, cur) => Math.abs(cur - n) < Math.abs(best - n) ? cur : best, 500);
    }
