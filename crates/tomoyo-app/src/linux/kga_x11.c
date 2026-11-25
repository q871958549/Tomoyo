#include <gtk/gtk.h>
#include <gdk/gdk.h>
#include <gio/gio.h>
#include <gio/gdesktopappinfo.h>
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <X11/keysym.h>
#include <X11/extensions/XTest.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int tomoyo_wl_clip_active(void);
int tomoyo_wl_clip_has_mime(const char *mime);
int tomoyo_wl_clip_set(const char *m1, const char *m2, const uint8_t *data, size_t len);
uint8_t *tomoyo_wl_clip_read(const char *mime, size_t *out_len);

void tomoyo_linux_free_ptr(void *p) { if (p) free(p); }

int tomoyo_linux_launch_desktop(const char *desktop_path) {
    if (!desktop_path) return 0;
    GDesktopAppInfo *info = g_desktop_app_info_new_from_filename(desktop_path);
    if (!info) return 0;
    GdkDisplay *disp = gdk_display_get_default();
    GAppLaunchContext *ctx = disp ? G_APP_LAUNCH_CONTEXT(gdk_display_get_app_launch_context(disp)) : NULL;
    GError *err = NULL;
    gboolean ok = g_app_info_launch(G_APP_INFO(info), NULL, ctx, &err);
    if (err) g_error_free(err);
    if (ctx) g_object_unref(ctx);
    g_object_unref(info);
    return ok ? 1 : 0;
}

int tomoyo_linux_clipboard_set_text(const char *text) {
    if (!text) return 0;
    if (tomoyo_wl_clip_active()) {
        return tomoyo_wl_clip_set("text/plain;charset=utf-8", "UTF8_STRING", (const uint8_t *)text, strlen(text));
    }
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (!cb) return 0;
    gtk_clipboard_set_text(cb, text, -1);
    gtk_clipboard_store(cb);
    return 1;
}

char *tomoyo_linux_clipboard_get_text(void) {
    if (tomoyo_wl_clip_active()) {
        const char *mimes[] = { "text/plain;charset=utf-8", "UTF8_STRING", "text/plain" };
        for (int i = 0; i < 3; i++) {
            if (tomoyo_wl_clip_has_mime(mimes[i])) {
                size_t len = 0;
                uint8_t *raw = tomoyo_wl_clip_read(mimes[i], &len);
                if (raw && len > 0) return (char *)raw;
                free(raw);
            }
        }
        return NULL;
    }
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (!cb) return NULL;
    gchar *txt = gtk_clipboard_wait_for_text(cb);
    if (!txt) return NULL;
    char *copy = strdup(txt);
    g_free(txt);
    return copy;
}

int tomoyo_linux_clipboard_has_rich(void) {
    if (tomoyo_wl_clip_active()) {
        return (tomoyo_wl_clip_has_mime("text/uri-list") || tomoyo_wl_clip_has_mime("image/png")) ? 1 : 0;
    }
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (!cb) return 0;
    return (gtk_clipboard_wait_is_uris_available(cb) || gtk_clipboard_wait_is_image_available(cb)) ? 1 : 0;
}

int tomoyo_linux_clipboard_is_confidential(void) {
    if (tomoyo_wl_clip_active() && tomoyo_wl_clip_has_mime("x-kde-passwordManagerHint")) {
        size_t len = 0;
        uint8_t *dat = tomoyo_wl_clip_read("x-kde-passwordManagerHint", &len);
        int secret = (dat && len >= 6 && memcmp(dat, "secret", 6) == 0) ? 1 : 0;
        free(dat);
        return secret;
    }
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (!cb) return 0;
    GdkAtom hint = gdk_atom_intern_static_string("x-kde-passwordManagerHint");
    GtkSelectionData *sd = gtk_clipboard_wait_for_contents(cb, hint);
    if (!sd) return 0;
    gint len = 0;
    const guchar *dat = gtk_selection_data_get_data_with_length(sd, &len);
    int secret = (dat && len >= 6 && memcmp(dat, "secret", 6) == 0) ? 1 : 0;
    gtk_selection_data_free(sd);
    return secret;
}

char *tomoyo_linux_clipboard_get_uris(void) {
    if (tomoyo_wl_clip_active()) {
        if (!tomoyo_wl_clip_has_mime("text/uri-list")) return NULL;
        size_t len = 0;
        uint8_t *raw = tomoyo_wl_clip_read("text/uri-list", &len);
        if (raw && len > 0) return (char *)raw;
        free(raw);
        return NULL;
    }
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (!cb || !gtk_clipboard_wait_is_uris_available(cb)) return NULL;
    gchar **uris = gtk_clipboard_wait_for_uris(cb);
    if (!uris) return NULL;
    GString *gs = g_string_new("");
    for (int i = 0; uris[i]; i++) {
        g_string_append(gs, uris[i]);
        g_string_append_c(gs, '\n');
    }
    g_strfreev(uris);
    char *out = strdup(gs->str);
    g_string_free(gs, TRUE);
    return out;
}

uint8_t *tomoyo_linux_clipboard_get_wl_png(size_t *olen) {
    if (!tomoyo_wl_clip_active() || !tomoyo_wl_clip_has_mime("image/png")) return NULL;
    return tomoyo_wl_clip_read("image/png", olen);
}

uint8_t *tomoyo_linux_clipboard_get_image_rgba(uint32_t *ow, uint32_t *oh, size_t *olen) {
    GdkPixbuf *pb = NULL;
    if (tomoyo_wl_clip_active()) {
        if (!tomoyo_wl_clip_has_mime("image/png")) return NULL;
        size_t plen = 0;
        uint8_t *pbuf = tomoyo_wl_clip_read("image/png", &plen);
        if (!pbuf || plen == 0) { free(pbuf); return NULL; }
        GInputStream *is = g_memory_input_stream_new_from_data(pbuf, (gssize)plen, free);
        pb = gdk_pixbuf_new_from_stream(is, NULL, NULL);
        g_object_unref(is);
    } else {
        GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
        if (!cb || !gtk_clipboard_wait_is_image_available(cb)) return NULL;
        pb = gtk_clipboard_wait_for_image(cb);
    }
    if (!pb) return NULL;
    int w = gdk_pixbuf_get_width(pb), h = gdk_pixbuf_get_height(pb);
    int nc = gdk_pixbuf_get_n_channels(pb), rs = gdk_pixbuf_get_rowstride(pb);
    if (w <= 0 || h <= 0 || w > 4096 || h > 4096) { g_object_unref(pb); return NULL; }
    size_t sz = (size_t)w * (size_t)h * 4;
    uint8_t *rgba = (uint8_t *)malloc(sz);
    if (!rgba) { g_object_unref(pb); return NULL; }
    const guchar *px = gdk_pixbuf_get_pixels(pb);
    for (int y = 0; y < h; y++) {
        for (int x = 0; x < w; x++) {
            const guchar *s = px + y * rs + x * nc;
            size_t di = ((size_t)y * w + x) * 4;
            rgba[di] = s[0]; rgba[di + 1] = s[1]; rgba[di + 2] = s[2];
            rgba[di + 3] = nc >= 4 ? s[3] : 255;
        }
    }
    g_object_unref(pb);
    *ow = (uint32_t)w; *oh = (uint32_t)h; *olen = sz;
    return rgba;
}

int tomoyo_linux_clipboard_set_image_png(const char *path) {
    if (!path || !path[0]) return 0;
    if (tomoyo_wl_clip_active()) {
        FILE *f = fopen(path, "rb");
        if (!f) return 0;
        if (fseek(f, 0, SEEK_END) != 0) { fclose(f); return 0; }
        long flen = ftell(f);
        if (flen <= 0 || flen > 32L * 1024L * 1024L || fseek(f, 0, SEEK_SET) != 0) { fclose(f); return 0; }
        uint8_t *buf = (uint8_t *)malloc((size_t)flen);
        if (!buf) { fclose(f); return 0; }
        size_t nr = fread(buf, 1, (size_t)flen, f);
        fclose(f);
        int ok = (nr == (size_t)flen) ? tomoyo_wl_clip_set("image/png", NULL, buf, nr) : 0;
        free(buf);
        return ok;
    }
    GdkPixbuf *pb = gdk_pixbuf_new_from_file(path, NULL);
    if (!pb) return 0;
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (cb) {
        gtk_clipboard_set_image(cb, pb);
        gtk_clipboard_store(cb);
    }
    g_object_unref(pb);
    return cb ? 1 : 0;
}

int tomoyo_linux_clipboard_set_file_uri(const char *path) {
    if (!path || !path[0]) return 0;
    gchar *uri = g_filename_to_uri(path, NULL, NULL);
    if (!uri) return 0;
    char *list = g_strdup_printf("%s\r\n", uri);
    g_free(uri);
    int ok = 0;
    if (tomoyo_wl_clip_active()) {
        ok = tomoyo_wl_clip_set("text/uri-list", "text/plain;charset=utf-8", (const uint8_t *)list, strlen(list));
    } else {
        GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
        if (cb) {
            gtk_clipboard_set_text(cb, list, -1);
            gtk_clipboard_store(cb);
            ok = 1;
        }
    }
    g_free(list);
    return ok;
}

char *tomoyo_linux_active_app_name(void) {
    Display *dpy = XOpenDisplay(NULL);
    if (!dpy) return NULL;
    Window root = DefaultRootWindow(dpy);
    Atom a_act = XInternAtom(dpy, "_NET_ACTIVE_WINDOW", True);
    Atom a_pid = XInternAtom(dpy, "_NET_WM_PID", True);
    char *res = NULL;
    if (a_act != None && a_pid != None) {
        Atom rt; int rf; unsigned long n, ba; unsigned char *prop = NULL;
        if (XGetWindowProperty(dpy, root, a_act, 0, 1, False, XA_WINDOW, &rt, &rf, &n, &ba, &prop) == Success && prop && n >= 1) {
            Window aw = *(Window *)prop;
            XFree(prop); prop = NULL;
            if (aw && XGetWindowProperty(dpy, aw, a_pid, 0, 1, False, XA_CARDINAL, &rt, &rf, &n, &ba, &prop) == Success && prop && n >= 1) {
                unsigned long pid = *(unsigned long *)prop;
                char pbuf[64]; snprintf(pbuf, sizeof(pbuf), "/proc/%lu/comm", pid);
                FILE *f = fopen(pbuf, "r");
                if (f) {
                    char line[128] = {0};
                    if (fgets(line, sizeof(line), f)) {
                        size_t L = strlen(line);
                        if (L > 0 && line[L - 1] == '\n') line[L - 1] = '\0';
                        res = strdup(line);
                    }
                    fclose(f);
                }
            }
            if (prop) XFree(prop);
        }
    }
    XCloseDisplay(dpy);
    return res;
}

void tomoyo_linux_send_ctrl_chord(char key) {
    Display *dpy = XOpenDisplay(NULL);
    if (!dpy) return;
    KeyCode ctrl = XKeysymToKeycode(dpy, XK_Control_L);
    KeyCode kc = XKeysymToKeycode(dpy, (KeySym)(unsigned char)key);
    if (ctrl && kc) {
        XTestFakeKeyEvent(dpy, ctrl, True, 0);
        XTestFakeKeyEvent(dpy, kc, True, 0);
        XTestFakeKeyEvent(dpy, kc, False, 0);
        XTestFakeKeyEvent(dpy, ctrl, False, 0);
        XFlush(dpy);
    }
    XCloseDisplay(dpy);
}

typedef void (*kga_fire_cb)(void *ctx, const char *action_id);
typedef struct { GDBusConnection *bus; guint sub_id; kga_fire_cb cb; void *ctx; } kga_ctx_t;

static void on_kga_signal(GDBusConnection *c, const gchar *sender, const gchar *path, const gchar *iface, const gchar *sig, GVariant *params, gpointer ud) {
    (void)c; (void)sender; (void)path; (void)iface; (void)sig;
    kga_ctx_t *k = (kga_ctx_t *)ud;
    const gchar *comp = NULL, *act = NULL;
    gint64 ts = 0;
    g_variant_get(params, "(&s&sx)", &comp, &act, &ts);
    if (k && k->cb && comp && act && strcmp(comp, "tomoyo") == 0) k->cb(k->ctx, act);
}

void *tomoyo_kga_connect(kga_fire_cb cb, void *ctx) {
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) { if (err) g_error_free(err); return NULL; }
    kga_ctx_t *k = (kga_ctx_t *)calloc(1, sizeof(kga_ctx_t));
    k->bus = bus; k->cb = cb; k->ctx = ctx;
    k->sub_id = g_dbus_connection_signal_subscribe(
        bus, "org.kde.kglobalaccel", "org.kde.kglobalaccel.Component", "globalShortcutPressed",
        "/component/tomoyo", "tomoyo", G_DBUS_SIGNAL_FLAGS_NONE, on_kga_signal, k, NULL
    );
    return k;
}

static int kga_query_action(
    GDBusConnection *bus, int qt_key,
    char *comp_out, size_t clen, char *act_out, size_t alen,
    char *app_out, size_t aplen, char *fr_out, size_t flen
) {
    if (comp_out && clen) comp_out[0] = '\0';
    if (act_out && alen) act_out[0] = '\0';
    if (app_out && aplen) app_out[0] = '\0';
    if (fr_out && flen) fr_out[0] = '\0';
    GError *err = NULL;
    GVariant *ov = g_dbus_connection_call_sync(
        bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel",
        "action", g_variant_new("(i)", qt_key), G_VARIANT_TYPE("(as)"),
        G_DBUS_CALL_FLAGS_NONE, 250, NULL, &err
    );
    if (!ov) { if (err) g_error_free(err); return -1; }
    GVariantIter *it = NULL; g_variant_get(ov, "(as)", &it);
    const gchar *parts[4] = {0}; int idx = 0; const gchar *s = NULL;
    while (idx < 4 && g_variant_iter_next(it, "&s", &s)) parts[idx++] = s;
    int owned = (idx >= 1 && parts[0] && parts[0][0]);
    if (owned) {
        if (comp_out && clen) snprintf(comp_out, clen, "%s", parts[0]);
        if (act_out && alen && idx >= 2 && parts[1]) snprintf(act_out, alen, "%s", parts[1]);
        if (app_out && aplen && idx >= 3 && parts[2]) snprintf(app_out, aplen, "%s", parts[2]);
        if (fr_out && flen && idx >= 4 && parts[3]) snprintf(fr_out, flen, "%s", parts[3]);
    }
    g_variant_iter_free(it); g_variant_unref(ov);
    return owned ? 1 : 0;
}

static int kga_strip_foreign_key(
    GDBusConnection *bus, const char *comp, const char *act, const char *app, const char *fr, int qt_key
) {
    GVariantBuilder oa; g_variant_builder_init(&oa, G_VARIANT_TYPE("as"));
    g_variant_builder_add(&oa, "s", comp);
    g_variant_builder_add(&oa, "s", act[0] ? act : comp);
    g_variant_builder_add(&oa, "s", app[0] ? app : comp);
    g_variant_builder_add(&oa, "s", fr[0] ? fr : comp);
    GVariant *cur = g_dbus_connection_call_sync(
        bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel",
        "shortcut", g_variant_new("(as)", &oa), G_VARIANT_TYPE("(ai)"),
        G_DBUS_CALL_FLAGS_NONE, 250, NULL, NULL
    );
    if (!cur) return 0;
    GVariantIter *it = NULL; gint32 k = 0;
    GVariantBuilder oa2, nk;
    g_variant_builder_init(&oa2, G_VARIANT_TYPE("as"));
    g_variant_builder_add(&oa2, "s", comp);
    g_variant_builder_add(&oa2, "s", act[0] ? act : comp);
    g_variant_builder_add(&oa2, "s", app[0] ? app : comp);
    g_variant_builder_add(&oa2, "s", fr[0] ? fr : comp);
    g_variant_builder_init(&nk, G_VARIANT_TYPE("ai"));
    g_variant_get(cur, "(ai)", &it);
    while (g_variant_iter_next(it, "i", &k)) {
        if (k != qt_key) g_variant_builder_add(&nk, "i", k);
    }
    g_variant_iter_free(it); g_variant_unref(cur);
    GVariant *r = g_dbus_connection_call_sync(
        bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel",
        "setForeignShortcut", g_variant_new("(asai)", &oa2, &nk),
        NULL, G_DBUS_CALL_FLAGS_NONE, 300, NULL, NULL
    );
    if (r) { g_variant_unref(r); return 1; }
    return 0;
}

void tomoyo_kga_unblock_global(void *hk) {
    kga_ctx_t *k = (kga_ctx_t *)hk;
    if (!k || !k->bus) return;
    GVariant *r = g_dbus_connection_call_sync(k->bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel", "blockGlobalShortcuts", g_variant_new("(b)", FALSE), NULL, G_DBUS_CALL_FLAGS_NONE, 200, NULL, NULL);
    if (r) g_variant_unref(r);
}

void tomoyo_kga_purge_all(void *hk) {
    kga_ctx_t *k = (kga_ctx_t *)hk;
    if (!k || !k->bus) return;
    GVariantBuilder q; g_variant_builder_init(&q, G_VARIANT_TYPE("as"));
    g_variant_builder_add(&q, "s", "tomoyo");
    for (int i = 0; i < 3; i++) g_variant_builder_add(&q, "s", "");
    GVariant *res = g_dbus_connection_call_sync(k->bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel", "allActionsForComponent", g_variant_new("(as)", &q), G_VARIANT_TYPE("(aas)"), G_DBUS_CALL_FLAGS_NONE, 300, NULL, NULL);
    if (!res) return;
    GVariantIter *outer = NULL, *inner = NULL;
    g_variant_get(res, "(aas)", &outer);
    while (g_variant_iter_next(outer, "as", &inner)) {
        const gchar *parts[4] = {0}; int idx = 0; const gchar *s = NULL;
        while (idx < 4 && g_variant_iter_next(inner, "&s", &s)) parts[idx++] = s;
        if (idx >= 2 && parts[0] && strcmp(parts[0], "tomoyo") == 0 && parts[1] && parts[1][0]) {
            GVariantBuilder ia; g_variant_builder_init(&ia, G_VARIANT_TYPE("as"));
            for (int i = 0; i < idx; i++) g_variant_builder_add(&ia, "s", parts[i]);
            GVariant *ri = g_dbus_connection_call_sync(k->bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel", "setInactive", g_variant_new("(as)", &ia), NULL, G_DBUS_CALL_FLAGS_NONE, 150, NULL, NULL);
            if (ri) g_variant_unref(ri);
            GVariant *ru = g_dbus_connection_call_sync(k->bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel", "unregister", g_variant_new("(ss)", "tomoyo", parts[1]), G_VARIANT_TYPE("(b)"), G_DBUS_CALL_FLAGS_NONE, 150, NULL, NULL);
            if (ru) g_variant_unref(ru);
        }
        g_variant_iter_free(inner);
    }
    g_variant_iter_free(outer); g_variant_unref(res);
}

int tomoyo_kga_register(void *hk, const char *act_id, const char *friendly, int qt_key, int allow_override, char *owner_out, size_t olen) {
    if (owner_out && olen > 0) owner_out[0] = '\0';
    kga_ctx_t *k = (kga_ctx_t *)hk;
    if (!k || !k->bus) return -1;
    char oc[128], oa[128], oapp[128], ofr[128];
    int qrc = kga_query_action(k->bus, qt_key, oc, sizeof(oc), oa, sizeof(oa), oapp, sizeof(oapp), ofr, sizeof(ofr));
    if (qrc < 0) return -1;
    int stolen = 0;
    if (qrc == 1 && strcmp(oc, "tomoyo") == 0 && oa[0] && strcmp(oa, act_id) != 0) {
        GVariant *ru = g_dbus_connection_call_sync(k->bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel", "unregister", g_variant_new("(ss)", "tomoyo", oa), G_VARIANT_TYPE("(b)"), G_DBUS_CALL_FLAGS_NONE, 200, NULL, NULL);
        if (ru) g_variant_unref(ru);
    } else if (qrc == 1 && strcmp(oc, "tomoyo") != 0) {
        if (owner_out && olen > 0) snprintf(owner_out, olen, "%s (%s)", oapp[0] ? oapp : oc, oc);
        if (!allow_override) return 0;
        stolen = kga_strip_foreign_key(k->bus, oc, oa, oapp, ofr, qt_key);
    }
    const gchar *aid[5] = { "tomoyo", act_id, "Tomoyo", friendly, NULL };
    GVariantBuilder b1; g_variant_builder_init(&b1, G_VARIANT_TYPE("as"));
    for (int i = 0; i < 4; i++) g_variant_builder_add(&b1, "s", aid[i]);
    GVariant *r1 = g_dbus_connection_call_sync(k->bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel", "doRegister", g_variant_new("(as)", &b1), NULL, G_DBUS_CALL_FLAGS_NONE, 250, NULL, NULL);
    if (r1) g_variant_unref(r1);
    GVariantBuilder b2, b3; g_variant_builder_init(&b2, G_VARIANT_TYPE("as")); g_variant_builder_init(&b3, G_VARIANT_TYPE("ai"));
    for (int i = 0; i < 4; i++) g_variant_builder_add(&b2, "s", aid[i]);
    g_variant_builder_add(&b3, "i", qt_key);
    GVariant *r2 = g_dbus_connection_call_sync(k->bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel", "setShortcut", g_variant_new("(asaiu)", &b2, &b3, 6u), G_VARIANT_TYPE("(ai)"), G_DBUS_CALL_FLAGS_NONE, 250, NULL, NULL);
    if (r2) g_variant_unref(r2);
    char vc[128], va[128], vapp[128], vfr[128];
    if (kga_query_action(k->bus, qt_key, vc, sizeof(vc), va, sizeof(va), vapp, sizeof(vapp), vfr, sizeof(vfr)) == 1
        && strcmp(vc, "tomoyo") == 0 && strcmp(va, act_id) == 0) {
        return stolen ? 2 : 1;
    }
    if (vc[0] && owner_out && olen > 0) snprintf(owner_out, olen, "%s (%s)", vapp[0] ? vapp : vc, vc);
    return 0;
}

void tomoyo_kga_unregister(void *hk, const char *act_id) {
    kga_ctx_t *k = (kga_ctx_t *)hk;
    if (!k || !k->bus || !act_id) return;
    GVariant *r = g_dbus_connection_call_sync(k->bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel", "unregister", g_variant_new("(ss)", "tomoyo", act_id), G_VARIANT_TYPE("(b)"), G_DBUS_CALL_FLAGS_NONE, 200, NULL, NULL);
    if (r) g_variant_unref(r);
}

void tomoyo_kga_destroy(void *hk) {
    kga_ctx_t *k = (kga_ctx_t *)hk;
    if (!k) return;
    if (k->bus && k->sub_id) g_dbus_connection_signal_unsubscribe(k->bus, k->sub_id);
    if (k->bus) g_object_unref(k->bus);
    free(k);
}
