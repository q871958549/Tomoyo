    // --- Settings Center Bootstrap Prelude (loads before all feature modules — zero TDZ) ---
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
    function esc(s) {
      return String(s || '').replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
    }
    function post(msg) {
      if (window.chrome && window.chrome.webview) {
        window.chrome.webview.postMessage(JSON.stringify(msg));
      }
    }
    function setToast(text, isWarn) {
      const el = document.getElementById('toast');
      if (!el) return;
      el.classList.toggle('warn', !!isWarn);
      el.textContent = text;
    }
