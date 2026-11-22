#include <gtk/gtk.h>
#include <webkit2/webkit2.h>
#include <gio/gio.h>
#include <gdk/gdkkeysyms.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef void (*wv_msg_cb)(void *ctx, const char *json);
typedef int (*wv_key_cb)(void *ctx, uint32_t keyval, uint32_t state);
typedef int (*wv_uri_cb)(void *ctx, const char *scheme, const char *path, uint8_t **out_data, size_t *out_len);
typedef void (*wv_void_cb)(void *ctx);

typedef struct {
    GtkWidget *window;
    GtkWidget *header;
    GtkCssProvider *css;
    WebKitWebView *webview;
    wv_msg_cb on_msg;
    wv_key_cb on_key;
    wv_uri_cb on_uri;
    wv_void_cb on_focus_in;
    wv_void_cb on_close;
    void *ctx;
    int capture_hotkeys;
    int topmost;
} tomoyo_wv_t;

static const char *BRIDGE_INIT_JS =
    "(function(){"
    "if(!window.chrome)window.chrome={};"
    "if(!window.chrome.webview){"
    "const L=[];"
    "window.chrome.webview={"
    "postMessage:function(m){try{window.webkit.messageHandlers.tomoyo.postMessage(typeof m==='string'?m:JSON.stringify(m));}catch(_){}},"
    "addEventListener:function(t,f){if(t==='message'&&typeof f==='function')L.push(f);},"
    "removeEventListener:function(t,f){if(t==='message'){const i=L.indexOf(f);if(i>=0)L.splice(i,1);}}"
    "};"
    "window.__tomoyoDispatchHostEvent=function(raw){"
    "let d=raw;try{d=JSON.parse(raw);}catch(_){}"
    "L.slice().forEach(fn=>{try{fn({data:d});}catch(e){console.error(e);}});"
    "};"
    "}"
    "})();";

static void on_script_message(WebKitUserContentManager *mgr, WebKitJavascriptResult *res, gpointer ud) {
    (void)mgr;
    tomoyo_wv_t *h = (tomoyo_wv_t *)ud;
    if (!h || !h->on_msg) return;
    JSCValue *val = webkit_javascript_result_get_js_value(res);
    gchar *txt = jsc_value_to_string(val);
    if (txt) {
        h->on_msg(h->ctx, txt);
        g_free(txt);
    }
}

static void on_uri_scheme(WebKitURISchemeRequest *req, gpointer ud) {
    tomoyo_wv_t *h = (tomoyo_wv_t *)ud;
    const gchar *scheme = webkit_uri_scheme_request_get_scheme(req);
    const gchar *path = webkit_uri_scheme_request_get_path(req);
    if (!h || !h->on_uri) {
        GError *e = g_error_new_literal(g_quark_from_static_string("tomoyo"), 404, "not found");
        webkit_uri_scheme_request_finish_error(req, e);
        g_error_free(e);
        return;
    }
    uint8_t *buf = NULL;
    size_t len = 0;
    if (h->on_uri(h->ctx, scheme ? scheme : "", path ? path : "", &buf, &len) && buf && len > 0) {
        GBytes *bytes = g_bytes_new_take(buf, len);
        GInputStream *stream = g_memory_input_stream_new_from_bytes(bytes);
        const char *mime = "image/png";
        if (path) {
            if (g_str_has_suffix(path, ".jpg") || g_str_has_suffix(path, ".jpeg")) mime = "image/jpeg";
            else if (g_str_has_suffix(path, ".webp")) mime = "image/webp";
            else if (g_str_has_suffix(path, ".gif")) mime = "image/gif";
            else if (g_str_has_suffix(path, ".svg")) mime = "image/svg+xml";
        }
        webkit_uri_scheme_request_finish(req, stream, (gint64)len, mime);
        g_object_unref(stream);
        g_bytes_unref(bytes);
    } else {
        if (buf) free(buf);
        GError *e = g_error_new_literal(g_quark_from_static_string("tomoyo"), 404, "not found");
        webkit_uri_scheme_request_finish_error(req, e);
        g_error_free(e);
    }
}

static gboolean on_key_press(GtkWidget *w, GdkEventKey *ev, gpointer ud) {
    (void)w;
    tomoyo_wv_t *h = (tomoyo_wv_t *)ud;
    if (!h || !h->on_key) return FALSE;
    return h->on_key(h->ctx, (uint32_t)ev->keyval, (uint32_t)ev->state) ? TRUE : FALSE;
}

static gboolean on_focus_in_ev(GtkWidget *w, GdkEventFocus *ev, gpointer ud) {
    (void)w; (void)ev;
    tomoyo_wv_t *h = (tomoyo_wv_t *)ud;
    if (h && h->on_focus_in) h->on_focus_in(h->ctx);
    return FALSE;
}

static gboolean on_delete_ev(GtkWidget *w, GdkEvent *ev, gpointer ud) {
    (void)w; (void)ev;
    tomoyo_wv_t *h = (tomoyo_wv_t *)ud;
    if (h && h->on_close) h->on_close(h->ctx);
    gtk_main_quit();
    return FALSE;
}

void tomoyo_wv_apply_theme(tomoyo_wv_t *h, int mode) {
    GtkSettings *st = gtk_settings_get_default();
    if (st) g_object_set(st, "gtk-application-prefer-dark-theme", mode != 0 ? TRUE : FALSE, NULL);
    if (!h || !h->css) return;
    const char *bg = "#15161c", *fg = "#f3f4f6", *bd = "#272b38", *hov = "#2a2e3e";
    GdkRGBA wvbg = {0.08, 0.085, 0.11, 1.0};
    if (mode == 0) {
        bg = "#f7f8fa"; fg = "#111827"; bd = "#e5e7ee"; hov = "#ebeff5";
        wvbg = (GdkRGBA){0.97, 0.975, 0.99, 1.0};
    } else if (mode == 2) {
        bg = "#0f1320"; fg = "#eef2ff"; bd = "#1f2740"; hov = "#1c243a";
        wvbg = (GdkRGBA){0.043, 0.055, 0.09, 1.0};
    }
    char css[1024];
    snprintf(css, sizeof(css),
        "headerbar#tomoyo-hb { background: %s; background-image: none; color: %s; "
        "border-bottom: 1px solid %s; box-shadow: none; min-height: 36px; padding: 0 10px; } "
        "headerbar#tomoyo-hb .title { color: %s; font-size: 13px; font-weight: 600; } "
        "headerbar#tomoyo-hb button.titlebutton { color: %s; background: transparent; border: none; "
        "box-shadow: none; min-width: 24px; min-height: 24px; border-radius: 6px; padding: 2px; } "
        "headerbar#tomoyo-hb button.titlebutton:hover { background: %s; }",
        bg, fg, bd, fg, fg, hov);
    gtk_css_provider_load_from_data(h->css, css, -1, NULL);
    if (h->webview) webkit_web_view_set_background_color(h->webview, &wvbg);
}

tomoyo_wv_t *tomoyo_wv_create(
    const char *title,
    int width,
    int height,
    int mode,
    const uint8_t *icon_png,
    size_t icon_len,
    const char *extra_init_js,
    wv_msg_cb on_msg,
    wv_key_cb on_key,
    wv_uri_cb on_uri,
    wv_void_cb on_focus_in,
    wv_void_cb on_close,
    void *ctx
) {
    if (!gtk_init_check(NULL, NULL)) return NULL;

    tomoyo_wv_t *h = (tomoyo_wv_t *)calloc(1, sizeof(tomoyo_wv_t));
    if (!h) return NULL;
    h->on_msg = on_msg;
    h->on_key = on_key;
    h->on_uri = on_uri;
    h->on_focus_in = on_focus_in;
    h->on_close = on_close;
    h->ctx = ctx;

    h->window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(h->window), title ? title : "Tomoyo");
    gtk_window_set_default_size(GTK_WINDOW(h->window), width > 200 ? width : 960, height > 200 ? height : 670);
    gtk_window_set_position(GTK_WINDOW(h->window), GTK_WIN_POS_CENTER);
    h->header = gtk_header_bar_new();
    gtk_widget_set_name(h->header, "tomoyo-hb");
    gtk_header_bar_set_title(GTK_HEADER_BAR(h->header), title ? title : "Tomoyo");
    gtk_header_bar_set_show_close_button(GTK_HEADER_BAR(h->header), TRUE);
    gtk_window_set_titlebar(GTK_WINDOW(h->window), h->header);
    h->css = gtk_css_provider_new();
    gtk_style_context_add_provider(gtk_widget_get_style_context(h->header), GTK_STYLE_PROVIDER(h->css), GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);

    if (icon_png && icon_len > 0) {
        GInputStream *is = g_memory_input_stream_new_from_data(icon_png, (gssize)icon_len, NULL);
        GdkPixbuf *pb = gdk_pixbuf_new_from_stream(is, NULL, NULL);
        if (pb) {
            gtk_window_set_icon(GTK_WINDOW(h->window), pb);
            g_object_unref(pb);
        }
        g_object_unref(is);
    }

    WebKitWebContext *wctx = webkit_web_context_get_default();
    webkit_web_context_register_uri_scheme(wctx, "tomoyo-clip-img", on_uri_scheme, h, NULL);
    webkit_web_context_register_uri_scheme(wctx, "tomoyo-ocr-img", on_uri_scheme, h, NULL);
    WebKitSecurityManager *sm = webkit_web_context_get_security_manager(wctx);
    webkit_security_manager_register_uri_scheme_as_secure(sm, "tomoyo-clip-img");
    webkit_security_manager_register_uri_scheme_as_secure(sm, "tomoyo-ocr-img");
    webkit_security_manager_register_uri_scheme_as_cors_enabled(sm, "tomoyo-clip-img");
    webkit_security_manager_register_uri_scheme_as_cors_enabled(sm, "tomoyo-ocr-img");

    WebKitUserContentManager *ucm = webkit_user_content_manager_new();
    webkit_user_content_manager_register_script_message_handler(ucm, "tomoyo");
    g_signal_connect(ucm, "script-message-received::tomoyo", G_CALLBACK(on_script_message), h);

    WebKitUserScript *s0 = webkit_user_script_new(
        BRIDGE_INIT_JS, WEBKIT_USER_CONTENT_INJECT_ALL_FRAMES,
        WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START, NULL, NULL
    );
    webkit_user_content_manager_add_script(ucm, s0);
    webkit_user_script_unref(s0);

    if (extra_init_js && extra_init_js[0]) {
        WebKitUserScript *s1 = webkit_user_script_new(
            extra_init_js, WEBKIT_USER_CONTENT_INJECT_ALL_FRAMES,
            WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START, NULL, NULL
        );
        webkit_user_content_manager_add_script(ucm, s1);
        webkit_user_script_unref(s1);
    }

    h->webview = WEBKIT_WEB_VIEW(webkit_web_view_new_with_user_content_manager(ucm));
    WebKitSettings *ws = webkit_web_view_get_settings(h->webview);
    webkit_settings_set_enable_developer_extras(ws, FALSE);
    webkit_settings_set_default_font_family(ws, "阿里巴巴普惠体 3.0 55 Regular");
    tomoyo_wv_apply_theme(h, mode);

    gtk_container_add(GTK_CONTAINER(h->window), GTK_WIDGET(h->webview));
    g_signal_connect(h->window, "key-press-event", G_CALLBACK(on_key_press), h);
    g_signal_connect(h->window, "focus-in-event", G_CALLBACK(on_focus_in_ev), h);
    g_signal_connect(h->window, "delete-event", G_CALLBACK(on_delete_ev), h);

    g_object_unref(ucm);
    return h;
}

void tomoyo_wv_load_html(tomoyo_wv_t *h, const char *html) {
    if (!h || !h->webview || !html) return;
    webkit_web_view_load_html(h->webview, html, "https://tomoyo.local/");
}

void tomoyo_wv_load_uri(tomoyo_wv_t *h, const char *uri) {
    if (!h || !h->webview || !uri) return;
    webkit_web_view_load_uri(h->webview, uri);
}

void tomoyo_wv_show_and_run(tomoyo_wv_t *h) {
    if (!h || !h->window) return;
    gtk_widget_show_all(h->window);
    gtk_window_present(GTK_WINDOW(h->window));
    gtk_main();
}

void tomoyo_wv_eval_js(tomoyo_wv_t *h, const char *js) {
    if (!h || !h->webview || !js) return;
    webkit_web_view_evaluate_javascript(h->webview, js, -1, NULL, NULL, NULL, NULL, NULL);
}

static int activate_via_kwin_runner(const char *title) {
    if (!title || !title[0]) return 0;
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) {
        if (err) g_error_free(err);
        return 0;
    }
    GVariant *res = g_dbus_connection_call_sync(
        bus, "org.kde.KWin", "/WindowsRunner", "org.kde.krunner1", "Match",
        g_variant_new("(s)", title), G_VARIANT_TYPE("(a(sssuda{sv}))"),
        G_DBUS_CALL_FLAGS_NONE, 250, NULL, &err
    );
    if (!res) {
        if (err) g_error_free(err);
        g_object_unref(bus);
        return 0;
    }
    GVariantIter *iter = NULL;
    g_variant_get(res, "(a(sssuda{sv}))", &iter);
    gchar *best_id = NULL;
    const gchar *id = NULL, *mtitle = NULL, *icon = NULL;
    guint32 typ = 0;
    gdouble rel = 0.0;
    GVariant *props = NULL;
    while (g_variant_iter_next(iter, "(&s&s&sud@a{sv})", &id, &mtitle, &icon, &typ, &rel, &props)) {
        if (id && mtitle && strcmp(mtitle, title) == 0) {
            best_id = g_strdup(id);
            if (props) g_variant_unref(props);
            break;
        }
        if (props) g_variant_unref(props);
    }
    g_variant_iter_free(iter);
    g_variant_unref(res);
    int ok = 0;
    if (best_id) {
        GVariant *run_res = g_dbus_connection_call_sync(
            bus, "org.kde.KWin", "/WindowsRunner", "org.kde.krunner1", "Run",
            g_variant_new("(ss)", best_id, ""), NULL, G_DBUS_CALL_FLAGS_NONE, 250, NULL, &err
        );
        if (err) { g_error_free(err); err = NULL; }
        if (run_res) { g_variant_unref(run_res); ok = 1; }
        g_free(best_id);
    }
    g_object_unref(bus);
    return ok;
}

void tomoyo_wv_present(tomoyo_wv_t *h) {
    if (!h || !h->window) return;
    if (!gtk_widget_get_visible(h->window)) gtk_widget_show_all(h->window);
    const char *title = gtk_window_get_title(GTK_WINDOW(h->window));
    if (activate_via_kwin_runner(title)) return;
    const char *wl = getenv("WAYLAND_DISPLAY");
    if (wl && wl[0]) {
        gtk_widget_hide(h->window);
        gtk_window_deiconify(GTK_WINDOW(h->window));
        gtk_widget_show_all(h->window);
    } else {
        gtk_window_deiconify(GTK_WINDOW(h->window));
    }
    gtk_window_present_with_time(GTK_WINDOW(h->window), GDK_CURRENT_TIME);
}

void tomoyo_wv_close(tomoyo_wv_t *h) {
    if (h && h->window) gtk_window_close(GTK_WINDOW(h->window));
}

int tomoyo_wv_toggle_topmost(tomoyo_wv_t *h) {
    if (!h || !h->window) return 0;
    h->topmost = !h->topmost;
    gtk_window_set_keep_above(GTK_WINDOW(h->window), h->topmost ? TRUE : FALSE);
    return h->topmost;
}

void tomoyo_wv_set_clipboard(const char *text) {
    if (!text) return;
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (cb) {
        gtk_clipboard_set_text(cb, text, -1);
        gtk_clipboard_store(cb);
    }
}

char *tomoyo_wv_pick_folder(tomoyo_wv_t *h, const char *title) {
    GtkWindow *parent = (h && h->window) ? GTK_WINDOW(h->window) : NULL;
    GtkFileChooserNative *dlg = gtk_file_chooser_native_new(
        title ? title : "选择目录", parent, GTK_FILE_CHOOSER_ACTION_SELECT_FOLDER, "选择", "取消"
    );
    char *res = NULL;
    if (gtk_native_dialog_run(GTK_NATIVE_DIALOG(dlg)) == GTK_RESPONSE_ACCEPT) {
        res = gtk_file_chooser_get_filename(GTK_FILE_CHOOSER(dlg));
    }
    g_object_unref(dlg);
    return res;
}

void tomoyo_wv_free_str(char *s) {
    if (s) g_free(s);
}

typedef struct {
    void (*fn)(void *);
    void *data;
} idle_task_t;

static gboolean on_idle_run(gpointer ud) {
    idle_task_t *t = (idle_task_t *)ud;
    if (t && t->fn) t->fn(t->data);
    free(t);
    return G_SOURCE_REMOVE;
}

void tomoyo_wv_schedule_idle(void (*fn)(void *), void *data) {
    idle_task_t *t = (idle_task_t *)malloc(sizeof(idle_task_t));
    if (!t) return;
    t->fn = fn;
    t->data = data;
    g_idle_add(on_idle_run, t);
}

void tomoyo_wv_destroy(tomoyo_wv_t *h) {
    if (!h) return;
    if (h->css) g_object_unref(h->css);
    free(h);
}
