    const BOOT = window.__TOMOYO_CLIP_INIT__ || {};
    let items = BOOT.items || [];
    let tags = BOOT.tags || [];
    let activeFilter = '';
    let selectedKey = BOOT.focus_key || '';
    let searchTimer = null;
    let clearArmed = false;

    const TYPE_FILTERS = [
      { tok: '', label: '全部记录' },
      { tok: '@pin', label: '★ 置顶收藏' },
      { tok: '@text', label: '文本' },
      { tok: '@img', label: '图片 / 截图' },
      { tok: '@link', label: '网址链接' },
      { tok: '@file', label: '文件 / 路径' },
      { tok: '@code', label: '代码 / JSON' },
      { tok: '@color', label: '色值' }
    ];

    function post(msg) {
      try {
        if (window.chrome && window.chrome.webview) {
          window.chrome.webview.postMessage(JSON.stringify(msg));
        }
      } catch (_) {}
    }

    window.addEventListener('error', (e) => {
      post({ cmd: 'report_js_error', message: String(e.message || 'error'), source: 'clip_workbench', line: e.lineno || 0 });
    });
    window.addEventListener('unhandledrejection', (e) => {
      post({ cmd: 'report_js_error', message: String(e.reason || 'rejection'), source: 'clip_workbench', line: 0 });
    });

    function esc(s) {
      return String(s || '').replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
    }
    function setToast(s) { document.getElementById('toast').textContent = s; }
    function fmtTime(ts) {
      if (!ts) return '-';
      const d = new Date(ts * 1000);
      const pad = n => String(n).padStart(2, '0');
      return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
    }

    function renderRail() {
      const tw = document.getElementById('rail-types');
      tw.innerHTML = '';
      TYPE_FILTERS.forEach(f => {
        const btn = document.createElement('button');
        btn.className = activeFilter === f.tok ? 'active' : '';
        btn.textContent = f.label;
        btn.onclick = () => { activeFilter = f.tok; requestReload(); };
        tw.appendChild(btn);
      });
      const tg = document.getElementById('rail-tags');
      tg.innerHTML = '';
      if (!tags.length) {
        tg.innerHTML = '<div style="padding:4px 8px;font-size:11.5px;color:var(--muted)">暂无标签（在右侧添加）</div>';
      } else {
        tags.forEach(t => {
          const tok = '#' + t.tag;
          const btn = document.createElement('button');
          btn.className = activeFilter === tok ? 'active' : '';
          btn.innerHTML = `<span>#${esc(t.tag)}</span><span style="opacity:0.75;font-size:11px">${t.count}</span>`;
          btn.onclick = () => { activeFilter = tok; requestReload(); };
          tg.appendChild(btn);
        });
      }
    }

    function renderList() {
      const box = document.getElementById('list-scroll');
      box.innerHTML = '';
      if (!items.length) {
        box.innerHTML = '<div style="padding:20px;text-align:center;color:var(--muted)">当前条件下无剪贴板记录</div>';
        renderDetail();
        return;
      }
      if (!selectedKey || !items.some(i => i.key === selectedKey)) {
        selectedKey = items[0].key;
      }
      items.forEach(it => {
        const card = document.createElement('div');
        card.className = 'item-card' + (it.key === selectedKey ? ' selected' : '');
        const star = it.pinned ? '★ ' : '';
        const tagPill = it.tag ? `<span class="pill">#${esc(it.tag)}</span>` : '';
        const thumb = (it.kind === 'image' && it.image_url)
          ? `<img class="item-thumb" src="${esc(it.image_url)}" loading="lazy" alt="" />` : '';
        card.innerHTML = `
          <div class="item-top">
            <span class="pill">${esc(it.subtype.toUpperCase())}</span>
            <span style="font-size:11px;color:var(--muted)">${fmtTime(it.ts)}</span>
          </div>
          <div class="item-prev">${esc(star + it.preview)}</div>
          ${thumb}
          <div class="item-meta">
            ${tagPill}
            ${it.source_app ? `<span>${esc(it.source_app)}</span>` : ''}
            ${it.note ? `<span>· ${esc(it.note)}</span>` : ''}
          </div>
        `;
        card.onclick = () => { selectedKey = it.key; renderList(); };
        card.ondblclick = () => { selectedKey = it.key; copyCurrent(false); };
        box.appendChild(card);
      });
      renderDetail();
    }

    function currentItem() {
      return items.find(i => i.key === selectedKey) || null;
    }

    function renderDetail() {
      const it = currentItem();
      document.getElementById('detail-empty').style.display = it ? 'none' : 'grid';
      document.getElementById('detail-wrap').style.display = it ? 'flex' : 'none';
      if (!it) return;
      document.getElementById('d-kind').textContent = it.subtype.toUpperCase();
      document.getElementById('d-src').textContent = it.source_app || '未知来源';
      document.getElementById('d-time').textContent = fmtTime(it.ts);
      document.getElementById('d-tag').value = it.tag || '';
      document.getElementById('d-note').value = it.note || '';
      document.getElementById('btn-pin').textContent = it.pinned ? '取消置顶' : '置顶';

      const ta = document.getElementById('d-text');
      const img = document.getElementById('d-img');
      const imgBar = document.getElementById('d-img-bar');
      const singleBtn = document.getElementById('btn-single');
      const trBtn = document.getElementById('btn-tr');
      if (it.kind === 'image') {
        ta.style.display = 'none';
        singleBtn.style.display = 'none';
        if (trBtn) trBtn.style.display = 'none';
        img.style.display = 'block';
        imgBar.style.display = 'flex';
        img.src = it.image_url || '';
      } else {
        ta.style.display = 'block';
        ta.readOnly = it.kind === 'files';
        ta.value = it.text || '';
        singleBtn.style.display = 'inline-block';
        if (trBtn) trBtn.style.display = it.kind === 'text' ? 'inline-block' : 'none';
        img.style.display = 'none';
        imgBar.style.display = 'none';
      }
    }

    function requestReload() {
      renderRail();
      const q = [activeFilter, (document.getElementById('q-input').value || '').trim()].filter(Boolean).join(' ');
      post({ cmd: 'clip_list', query: q });
    }
    function onSearchInput() {
      if (searchTimer) clearTimeout(searchTimer);
      searchTimer = setTimeout(requestReload, 120);
    }

    function saveDetail() {
      const it = currentItem();
      if (!it) return;
      const newText = it.kind === 'text' ? document.getElementById('d-text').value : null;
      const tag = document.getElementById('d-tag').value.trim();
      const note = document.getElementById('d-note').value.trim();
      post({ cmd: 'clip_update', id_key: it.key, text: newText, tag, note });
    }

    function copyCurrent(singleLine) {
      const it = currentItem();
      if (!it) return;
      if (it.kind === 'image' && it.image_path) {
        post({ cmd: 'copy_text', text: it.image_path });
        setToast('✓ 已复制图片路径到剪贴板（在主窗口按 Enter 可直接贴图）');
        return;
      }
      let txt = document.getElementById('d-text').value || it.text || '';
      if (singleLine) txt = txt.split(/\s+/).filter(Boolean).join(' ');
      post({ cmd: 'copy_text', text: txt });
      setToast(singleLine ? '✓ 已复制单行纯文本' : '✓ 已复制到剪贴板');
    }

    function togglePinCurrent() {
      const it = currentItem();
      if (!it) return;
      post({ cmd: 'clip_toggle_pin', id_key: it.key });
    }
    function deleteCurrent() {
      const it = currentItem();
      if (!it) return;
      post({ cmd: 'clip_delete', id_key: it.key });
    }
    function openCurrentImage() {
      const it = currentItem();
      if (it && it.image_path) post({ cmd: 'open_path', path: it.image_path });
    }
    function translateCurrent() {
      const it = currentItem();
      if (!it || it.kind !== 'text') return;
      const txt = (document.getElementById('d-text').value || it.text || '').trim();
      if (!txt) return;
      post({ cmd: 'open_translator', text: txt, target_lang: '' });
      setToast(`已发送 ${txt.length} 字到 Tomoyo 多源翻译`);
    }

    function clearUnpinned() {
      const btn = document.getElementById('btn-clear');
      if (!clearArmed) {
        clearArmed = true;
        btn.textContent = '再次点击确认清空';
        setTimeout(() => { clearArmed = false; btn.textContent = '清空非置顶'; }, 3500);
        return;
      }
      clearArmed = false;
      btn.textContent = '清空非置顶';
      post({ cmd: 'clip_clear_unpinned' });
    }

    function exportBackup() { post({ cmd: 'clip_export' }); }
    function openImportModal() { document.getElementById('import-modal').classList.add('open'); }
    function closeImportModal() { document.getElementById('import-modal').classList.remove('open'); }
    function confirmImport() {
      const raw = document.getElementById('import-json').value.trim();
      if (!raw) return;
      post({ cmd: 'clip_import', json: raw });
      closeImportModal();
    }

    function applyData(data) {
      items = data.items || [];
      tags = data.tags || [];
      document.getElementById('hdr-total').textContent = `${data.total ?? items.length} 条`;
      document.getElementById('hdr-cap').textContent = data.capture_enabled === false ? '已暂停采集' : '采集中';
      renderRail();
      renderList();
    }

    if (window.chrome && window.chrome.webview) {
      window.chrome.webview.addEventListener('message', (ev) => {
        let msg = ev.data;
        if (typeof msg === 'string') { try { msg = JSON.parse(msg); } catch (_) { return; } }
        if (!msg || !msg.event) return;
        if (msg.event === 'clip_list_loaded' && msg.data) {
          applyData(msg.data);
        } else if (msg.event === 'clip_mutated') {
          if (msg.key) selectedKey = msg.key;
          if (msg.ok === false) setToast('✗ ' + (msg.message || '操作失败'));
          else if (msg.action === 'update') setToast('✓ 已保存修改并同步主窗口');
          else if (msg.action === 'delete') setToast('✓ 已删除记录');
          else if (msg.action === 'clear') setToast(`✓ 已清理 ${msg.count || 0} 条非置顶记录`);
          else if (msg.action === 'import') setToast(`✓ 已导入 ${msg.count || 0} 条剪贴板记录`);
          else if (msg.action === 'pin') setToast(msg.pinned ? '★ 已置顶收藏' : '已取消置顶');
          requestReload();
        } else if (msg.event === 'clip_exported') {
          post({ cmd: 'copy_text', text: msg.json || '[]' });
          setToast('✓ 已将 JSON 备份复制到剪贴板（可直接保存为 .json 文件）');
        }
      });
    }

    window.addEventListener('keydown', (e) => {
      if (e.ctrlKey && (e.key === 's' || e.key === 'S')) { e.preventDefault(); saveDetail(); }
      else if (e.ctrlKey && e.shiftKey && (e.key === 'T' || e.key === 't')) { e.preventDefault(); post({ cmd: 'toggle_pin_topmost' }); }
      else if (e.key === 'Escape') {
        if (document.getElementById('import-modal').classList.contains('open')) closeImportModal();
        else post({ cmd: 'close' });
      }
    });

    (function init() {
      const th = (BOOT.theme || 'dark').toLowerCase();
      if (th === 'dark') document.body.classList.add('theme-dark');
      else if (th === 'midnight') document.body.classList.add('theme-midnight');
      applyData(BOOT);
    })();
