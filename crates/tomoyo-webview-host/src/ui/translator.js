    const BOOT = window.__TOMOYO_TR_INIT__ || {};
    let historyItems = Array.isArray(BOOT.history) ? BOOT.history : [];
    let wordbookItems = Array.isArray(BOOT.wordbook) ? BOOT.wordbook : [];
    let lastBatch = null;
    let drawerMode = null;
    let clearArmed = false;

    function post(msg) {
      try {
        if (window.chrome && window.chrome.webview) {
          window.chrome.webview.postMessage(JSON.stringify(msg));
        }
      } catch (_) {}
    }

    window.addEventListener('error', (e) => {
      post({ cmd: 'report_js_error', message: String(e.message || 'error'), source: 'translator', line: e.lineno || 0 });
    });
    window.addEventListener('unhandledrejection', (e) => {
      post({ cmd: 'report_js_error', message: String(e.reason || 'rejection'), source: 'translator', line: 0 });
    });

    function esc(s) {
      return String(s || '').replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
    }
    function setToast(s) { document.getElementById('toast').textContent = s; }

    function updateCharCount() {
      const len = (document.getElementById('src-text').value || '').length;
      document.getElementById('char-count').textContent = `${len} 字符`;
    }

    function isWordSaved(word) {
      const low = String(word || '').trim().toLowerCase();
      return !!low && wordbookItems.some(w => String(w.word || '').toLowerCase() === low);
    }

    function copyText(txt, label) {
      if (!txt) return;
      post({ cmd: 'copy_text', text: txt });
      setToast(`✓ 已复制${label || '译文'}（${txt.length} 字）`);
    }

    function speakText(txt, langHint) {
      const clean = String(txt || '').trim();
      if (!clean) return;
      if (!('speechSynthesis' in window)) {
        setToast('当前系统未启用语音合成组件');
        return;
      }
      try {
        window.speechSynthesis.cancel();
        const u = new SpeechSynthesisUtterance(clean.slice(0, 600));
        if (langHint === 'zh-Hans' || /[\u4e00-\u9fff]/.test(clean)) u.lang = 'zh-CN';
        else if (langHint === 'ja') u.lang = 'ja-JP';
        else if (langHint === 'ko') u.lang = 'ko-KR';
        else if (langHint === 'fr') u.lang = 'fr-FR';
        else if (langHint === 'de') u.lang = 'de-DE';
        else u.lang = 'en-US';
        window.speechSynthesis.speak(u);
        setToast('🔊 正在朗读...');
      } catch (_) {
        setToast('朗读启动失败');
      }
    }

    function clearSource() {
      const ta = document.getElementById('src-text');
      ta.value = '';
      ta.focus();
      updateCharCount();
      document.getElementById('results-box').innerHTML = '';
      lastBatch = null;
    }

    function swapLangs() {
      const src = document.getElementById('src-lang');
      const tgt = document.getElementById('tgt-lang');
      if (tgt.value === 'auto') {
        tgt.value = 'en';
      } else if (tgt.value === 'en') {
        tgt.value = 'zh-Hans';
      } else if (tgt.value === 'zh-Hans') {
        tgt.value = 'en';
      } else if (src.value !== 'auto') {
        const tmp = src.value;
        src.value = tgt.value;
        tgt.value = tmp;
      }
      if (document.getElementById('src-text').value.trim()) runTranslate();
    }

    function onLangChanged() {
      if (document.getElementById('src-text').value.trim()) runTranslate();
    }

    function runTranslate() {
      const text = document.getElementById('src-text').value.trim();
      if (!text) {
        setToast('请先输入或划选待翻译文本');
        return;
      }
      const source_lang = document.getElementById('src-lang').value || 'auto';
      const target_lang = document.getElementById('tgt-lang').value || 'auto';
      const btn = document.getElementById('btn-run');
      btn.disabled = true;
      btn.textContent = '翻译中...';
      document.getElementById('results-box').innerHTML = '<div class="card loading-row">正在并行查询已启用的翻译引擎...</div>';
      setToast('正在请求多源翻译引擎...');
      post({ cmd: 'run_translate', text, source_lang, target_lang });
    }

    function saveCardToWordbook(idx) {
      if (!lastBatch || !lastBatch.results || !lastBatch.results[idx]) return;
      const r = lastBatch.results[idx];
      const src = lastBatch.source_text || document.getElementById('src-text').value.trim();
      if (!src || !r.translated) return;
      post({
        cmd: 'tr_save_word',
        word: src,
        translation: r.translated,
        phonetic: r.phonetic || '',
        note: r.engine_name || ''
      });
    }

    function renderResults(batch) {
      lastBatch = batch;
      const box = document.getElementById('results-box');
      box.innerHTML = '';
      const results = (batch && Array.isArray(batch.results)) ? batch.results : [];
      if (!results.length) {
        box.innerHTML = '<div class="card loading-row">无翻译结果，请在「设置中心 → 翻译引擎」检查已启用的引擎</div>';
        return;
      }
      const saved = isWordSaved(batch.source_text);
      results.forEach((r, idx) => {
        const card = document.createElement('div');
        card.className = 'card';
        const det = r.detected_lang ? `<span class="pill">${esc(r.detected_lang)} → ${esc(batch.target_lang)}</span>` : `<span class="pill">→ ${esc(batch.target_lang)}</span>`;
        const ms = `<span class="pill">${r.elapsed_ms || 0} ms</span>`;
        let bodyHtml = '';
        if (r.ok && r.translated) {
          const dictLines = r.extra_dict ? String(r.extra_dict).split(/\r?\n/).filter(Boolean) : [];
          const dictHtml = (r.phonetic || dictLines.length)
            ? `<div class="dict-box">${r.phonetic ? `<div class="phonetic">音标：${esc(r.phonetic)}</div>` : ''}${dictLines.map(d => `<div>${esc(d)}</div>`).join('')}</div>`
            : '';
          bodyHtml = `<div class="eng-text">${esc(r.translated)}</div>${dictHtml}`;
        } else {
          bodyHtml = `<div class="eng-err">⚠ ${esc(r.error || '翻译失败')}</div>`;
        }
        const actionsHtml = (r.ok && r.translated)
          ? `<div style="display:flex;gap:5px">
               <button class="btn sm" onclick="copyText(lastBatch.results[${idx}].translated, ${JSON.stringify(r.engine_name + ' 译文')})">复制</button>
               <button class="btn sm ${saved ? 'active' : ''}" onclick="saveCardToWordbook(${idx})">${saved ? '★ 已收藏' : '☆ 生词本'}</button>
               <button class="btn sm" onclick="speakText(lastBatch.results[${idx}].translated, lastBatch.target_lang)">朗读</button>
             </div>`
          : '';
        card.innerHTML = `
          <div class="eng-head">
            <div class="eng-meta">
              <span class="eng-name">${esc(r.engine_name)}</span>
              ${det}
              ${ms}
            </div>
            ${actionsHtml}
          </div>
          ${bodyHtml}
        `;
        box.appendChild(card);
      });
    }

    function toggleDrawer(mode) {
      if (drawerMode === mode) { closeDrawer(); return; }
      drawerMode = mode;
      document.getElementById('drawer').classList.add('open');
      document.getElementById('btn-wb').classList.toggle('active', mode === 'wordbook');
      document.getElementById('btn-hist').classList.toggle('active', mode === 'history');
      document.getElementById('drawer-title').textContent = mode === 'wordbook'
        ? `生词本 (${wordbookItems.length})` : `翻译历史 (${historyItems.length})`;
      document.getElementById('btn-clear-hist').style.display = mode === 'history' ? 'inline-block' : 'none';
      renderDrawer();
    }

    function closeDrawer() {
      drawerMode = null;
      document.getElementById('drawer').classList.remove('open');
      document.getElementById('btn-wb').classList.remove('active');
      document.getElementById('btn-hist').classList.remove('active');
    }

    function renderDrawer() {
      if (!drawerMode) return;
      const list = document.getElementById('drawer-list');
      const q = (document.getElementById('drawer-q').value || '').trim().toLowerCase();
      list.innerHTML = '';
      if (drawerMode === 'wordbook') {
        document.getElementById('drawer-title').textContent = `生词本 (${wordbookItems.length})`;
        const rows = wordbookItems.filter(w => !q || w.word.toLowerCase().includes(q) || w.translation.toLowerCase().includes(q));
        if (!rows.length) {
          list.innerHTML = '<div class="loading-row">暂无收藏生词</div>';
          return;
        }
        rows.forEach(w => {
          const el = document.createElement('div');
          el.className = 'd-item';
          el.innerHTML = `
            <div class="d-src">${esc(w.word)} ${w.phonetic ? `<span style="color:var(--accent);font-weight:400">${esc(w.phonetic)}</span>` : ''}</div>
            <div class="d-tr">${esc(w.translation)}</div>
            <div class="d-foot">
              <span>${esc(w.note || '生词')}</span>
              <button class="btn sm danger" onclick="event.stopPropagation();post({cmd:'tr_delete_word',word:${JSON.stringify(w.word)}})">移除</button>
            </div>
          `;
          el.onclick = () => { loadAndTranslate(w.word, 'auto'); };
          list.appendChild(el);
        });
      } else {
        document.getElementById('drawer-title').textContent = `翻译历史 (${historyItems.length})`;
        const rows = historyItems.filter(h => !q || h.source.toLowerCase().includes(q) || h.translated.toLowerCase().includes(q));
        if (!rows.length) {
          list.innerHTML = '<div class="loading-row">暂无翻译历史</div>';
          return;
        }
        rows.forEach(h => {
          const el = document.createElement('div');
          el.className = 'd-item';
          el.innerHTML = `
            <div class="d-src">${esc(h.source)}</div>
            <div class="d-tr">${esc(h.translated)}</div>
            <div class="d-foot"><span>${esc(h.engine)} · → ${esc(h.target_lang)}</span></div>
          `;
          el.onclick = () => { loadAndTranslate(h.source, h.target_lang || 'auto'); };
          list.appendChild(el);
        });
      }
    }

    function clearHistory() {
      const btn = document.getElementById('btn-clear-hist');
      if (!clearArmed) {
        clearArmed = true;
        btn.textContent = '确认清空';
        setTimeout(() => { clearArmed = false; btn.textContent = '清空历史'; }, 3500);
        return;
      }
      clearArmed = false;
      btn.textContent = '清空历史';
      post({ cmd: 'tr_clear_history' });
    }

    function loadAndTranslate(text, targetLang) {
      document.getElementById('src-text').value = text || '';
      if (targetLang) document.getElementById('tgt-lang').value = targetLang;
      updateCharCount();
      if (text && text.trim()) runTranslate();
    }

    if (window.chrome && window.chrome.webview) {
      window.chrome.webview.addEventListener('message', (ev) => {
        let msg = ev.data;
        if (typeof msg === 'string') { try { msg = JSON.parse(msg); } catch (_) { return; } }
        if (!msg || !msg.event) return;
        if (msg.event === 'translate_result') {
          const btn = document.getElementById('btn-run');
          btn.disabled = false;
          btn.textContent = '翻译';
          if (msg.store) {
            if (Array.isArray(msg.store.history)) historyItems = msg.store.history;
            if (Array.isArray(msg.store.wordbook)) wordbookItems = msg.store.wordbook;
          }
          renderResults(msg.data);
          if (drawerMode) renderDrawer();
          const okCount = ((msg.data && msg.data.results) || []).filter(r => r.ok).length;
          setToast(`✓ 完成翻译（${okCount}/${((msg.data && msg.data.results) || []).length} 引擎成功 · ${msg.data.trace_id || ''}）`);
        } else if (msg.event === 'translate_reload_req') {
          if (msg.target_lang) document.getElementById('tgt-lang').value = msg.target_lang;
          if (typeof msg.text === 'string' && msg.text.trim()) {
            loadAndTranslate(msg.text, msg.target_lang || '');
          }
        } else if (msg.event === 'tr_store_updated') {
          if (Array.isArray(msg.history)) historyItems = msg.history;
          if (Array.isArray(msg.wordbook)) wordbookItems = msg.wordbook;
          if (lastBatch) renderResults(lastBatch);
          if (drawerMode) renderDrawer();
          if (msg.action === 'save_word') setToast('★ 已收藏到生词本');
          else if (msg.action === 'delete_word') setToast('已从生词本移除');
          else if (msg.action === 'clear_history') setToast('已清空翻译历史');
        }
      });
    }

    window.addEventListener('keydown', (e) => {
      if (e.ctrlKey && e.key === 'Enter') { e.preventDefault(); runTranslate(); }
      else if (e.ctrlKey && e.shiftKey && (e.key === 'T' || e.key === 't')) { e.preventDefault(); post({ cmd: 'toggle_pin_topmost' }); }
      else if (e.key === 'Escape') {
        if (drawerMode) closeDrawer();
        else post({ cmd: 'close' });
      }
    });

    (function init() {
      const th = (BOOT.theme || 'dark').toLowerCase();
      if (th === 'dark') document.body.classList.add('theme-dark');
      else if (th === 'midnight') document.body.classList.add('theme-midnight');
      const ta = document.getElementById('src-text');
      ta.addEventListener('input', updateCharCount);
      if (BOOT.target_lang) document.getElementById('tgt-lang').value = BOOT.target_lang;
      if (BOOT.text) {
        ta.value = BOOT.text;
        updateCharCount();
        runTranslate();
      } else {
        updateCharCount();
        ta.focus();
      }
    })();
