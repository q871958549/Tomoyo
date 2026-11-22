#define _GNU_SOURCE
#include <gtk/gtk.h>
#include <gio/gio.h>
#include <gio/gunixfdlist.h>
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int ensure_gtk_p(void) { return gtk_init_check(NULL, NULL) ? 1 : 0; }

char *tomoyo_cap_monitors_json(void) {
    if (!ensure_gtk_p()) return NULL;
    GdkDisplay *d = gdk_display_get_default();
    if (!d) return NULL;
    int n = gdk_display_get_n_monitors(d);
    GString *gs = g_string_new("[");
    for (int i = 0; i < n; i++) {
        GdkMonitor *m = gdk_display_get_monitor(d, i);
        GdkRectangle g;
        gdk_monitor_get_geometry(m, &g);
        int sc = gdk_monitor_get_scale_factor(m);
        g_string_append_printf(gs, "%s{\"x\":%d,\"y\":%d,\"w\":%d,\"h\":%d,\"s\":%d}", i ? "," : "", g.x, g.y, g.width, g.height, sc);
    }
    g_string_append_c(gs, ']');
    char *out = strdup(gs->str);
    g_string_free(gs, TRUE);
    return out;
}

typedef struct {
    GMainLoop *loop;
    char *json;
} kwin_probe_t;

static void on_kwin_call(
    GDBusConnection *c, const gchar *s, const gchar *p, const gchar *i,
    const gchar *m, GVariant *params, GDBusMethodInvocation *inv, gpointer ud
) {
    (void)c; (void)s; (void)p; (void)i;
    kwin_probe_t *kp = (kwin_probe_t *)ud;
    if (strcmp(m, "Report") == 0 && !kp->json) {
        const gchar *str = NULL;
        g_variant_get(params, "(&s)", &str);
        if (str) kp->json = strdup(str);
    }
    g_dbus_method_invocation_return_value(inv, NULL);
    if (kp->loop && g_main_loop_is_running(kp->loop)) g_main_loop_quit(kp->loop);
}

typedef struct {
    GMainLoop *lp;
    guint tid;
} loop_to_t;

static gboolean on_loop_timeout(gpointer ud) {
    loop_to_t *t = (loop_to_t *)ud;
    if (t) {
        t->tid = 0;
        if (t->lp && g_main_loop_is_running(t->lp)) g_main_loop_quit(t->lp);
    }
    return G_SOURCE_REMOVE;
}

static GDBusConnection *g_sc_bus = NULL;
static char *g_sc_sess = NULL;

static GDBusConnection *get_session_bus(void) {
    if (!g_sc_bus) g_sc_bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, NULL);
    return g_sc_bus;
}

static char *run_kwin_script(const char *js_body, const char *plugin_name, int wait_ms) {
    GDBusConnection *bus = get_session_bus();
    if (!bus) return NULL;
    static const GDBusInterfaceVTable vt = { on_kwin_call, NULL, NULL, {NULL} };
    kwin_probe_t kp = { g_main_loop_new(NULL, FALSE), NULL };
    GDBusNodeInfo *ni = g_dbus_node_info_new_for_xml(
        "<node><interface name='dev.tomoyo.CapKWin'><method name='Report'><arg type='s' name='d' direction='in'/></method></interface></node>",
        NULL);
    guint reg = ni ? g_dbus_connection_register_object(bus, "/TomoyoCapKWin", ni->interfaces[0], &vt, &kp, NULL, NULL) : 0;
    char *js_path = g_strdup_printf("%s/tomoyo_kw_%d_%s.js", g_get_tmp_dir(), (int)getpid(), plugin_name);
    FILE *f = fopen(js_path, "w");
    if (f) { fputs(js_body, f); fclose(f); }
    GVariant *loaded = f ? g_dbus_connection_call_sync(
        bus, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "loadScript",
        g_variant_new("(ss)", js_path, plugin_name), G_VARIANT_TYPE("(i)"), G_DBUS_CALL_FLAGS_NONE, 300, NULL, NULL) : NULL;
    int sid = -1;
    if (loaded) { g_variant_get(loaded, "(i)", &sid); g_variant_unref(loaded); }
    if (sid >= 0) {
        GVariant *sr = g_dbus_connection_call_sync(
            bus, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "start",
            NULL, NULL, G_DBUS_CALL_FLAGS_NONE, 300, NULL, NULL);
        if (sr) { g_variant_unref(sr); }
        if (wait_ms > 0) {
            loop_to_t to = { kp.loop, 0 };
            to.tid = g_timeout_add((guint)wait_ms, on_loop_timeout, &to);
            g_main_loop_run(kp.loop);
            if (to.tid) g_source_remove(to.tid);
        }
        GVariant *ur = g_dbus_connection_call_sync(
            bus, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "unloadScript",
            g_variant_new("(s)", plugin_name), G_VARIANT_TYPE("(b)"), G_DBUS_CALL_FLAGS_NONE, 200, NULL, NULL);
        if (ur) g_variant_unref(ur);
    }
    unlink(js_path); g_free(js_path);
    if (reg) g_dbus_connection_unregister_object(bus, reg);
    if (ni) g_dbus_node_info_unref(ni);
    g_main_loop_unref(kp.loop);
    return kp.json;
}

char *tomoyo_cap_kwin_snapshot_info(void) {
    GDBusConnection *bus = get_session_bus();
    if (!bus) return NULL;
    const gchar *uname = g_dbus_connection_get_unique_name(bus);
    char *js = g_strdup_printf(
        "const vs=workspace.virtualScreenGeometry,cp=workspace.cursorPos,dk=workspace.currentDesktop,s=workspace.stackingOrder,w=[];\n"
        "for(let i=s.length-1;i>=0;i--){const c=s[i];if(c.minimized||c.deleted||c.hidden||c.desktopWindow)continue;"
        "if(c.skipTaskbar&&!c.dock&&!c.dialog)continue;if(!c.normalWindow&&!c.dialog&&!c.dock)continue;"
        "if(!c.onAllDesktops&&c.desktops&&c.desktops.indexOf(dk)===-1)continue;const g=c.frameGeometry;"
        "if(g.width<24||g.height<20)continue;"
        "w.push({x:Math.round(g.x),y:Math.round(g.y),w:Math.round(g.width),h:Math.round(g.height),fs:c.fullScreen?1:0});}\n"
        "callDBus(\"%s\",\"/TomoyoCapKWin\",\"dev.tomoyo.CapKWin\",\"Report\","
        "JSON.stringify({vw:Math.round(vs.width),vh:Math.round(vs.height),vx:Math.round(vs.x),vy:Math.round(vs.y),cx:Math.round(cp.x),cy:Math.round(cp.y),wins:w}));\n",
        uname ? uname : "");
    char pname[48];
    snprintf(pname, sizeof(pname), "tomoyo_snap_%d", (int)getpid());
    char *res = run_kwin_script(js, pname, 220);
    g_free(js);
    return res;
}

uint8_t *tomoyo_cap_kwin_grab(int *ow, int *oh, size_t *olen) {
    GDBusConnection *bus = get_session_bus();
    if (!bus) return NULL;
    int pfd[2];
    if (pipe2(pfd, O_CLOEXEC) != 0) return NULL;
    GUnixFDList *fdl = g_unix_fd_list_new_from_array(&pfd[1], 1);
    GVariantBuilder b; g_variant_builder_init(&b, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&b, "{sv}", "include-cursor", g_variant_new_boolean(FALSE));
    g_variant_builder_add(&b, "{sv}", "native-resolution", g_variant_new_boolean(TRUE));
    GVariant *res = g_dbus_connection_call_with_unix_fd_list_sync(
        bus, "org.kde.KWin", "/org/kde/KWin/ScreenShot2", "org.kde.KWin.ScreenShot2", "CaptureWorkspace",
        g_variant_new("(a{sv}h)", &b, 0), G_VARIANT_TYPE("(a{sv})"), G_DBUS_CALL_FLAGS_NONE, 1500, fdl, NULL, NULL, NULL);
    g_object_unref(fdl);
    close(pfd[1]);
    if (!res) { close(pfd[0]); return NULL; }
    GVariant *dict = g_variant_get_child_value(res, 0);
    guint32 w = 0, h = 0, stride = 0, fmt = 0;
    g_variant_lookup(dict, "width", "u", &w);
    g_variant_lookup(dict, "height", "u", &h);
    g_variant_lookup(dict, "stride", "u", &stride);
    g_variant_lookup(dict, "format", "u", &fmt);
    g_variant_unref(dict);
    g_variant_unref(res);
    int is_rgba = (fmt == 16 || fmt == 17 || fmt == 18);
    int is_bgra = (fmt == 4 || fmt == 5 || fmt == 6);
    if (w == 0 || h == 0 || stride < w * 4 || (!is_rgba && !is_bgra)) { close(pfd[0]); return NULL; }
    size_t raw_len = (size_t)stride * (size_t)h;
    uint8_t *raw = (uint8_t *)malloc(raw_len);
    uint8_t *out = raw ? (uint8_t *)malloc((size_t)w * (size_t)h * 4) : NULL;
    if (!raw || !out) { free(raw); free(out); close(pfd[0]); return NULL; }
    size_t off = 0;
    while (off < raw_len) {
        ssize_t n = read(pfd[0], raw + off, raw_len - off);
        if (n <= 0) break;
        off += (size_t)n;
    }
    close(pfd[0]);
    if (off < raw_len) { free(raw); free(out); return NULL; }
    for (guint32 y = 0; y < h; y++) {
        const uint8_t *sr = raw + (size_t)y * stride;
        uint8_t *dr = out + (size_t)y * (size_t)w * 4;
        for (guint32 x = 0; x < w; x++) {
            size_t i = (size_t)x * 4;
            if (is_rgba) { dr[i] = sr[i + 2]; dr[i + 1] = sr[i + 1]; dr[i + 2] = sr[i]; }
            else { dr[i] = sr[i]; dr[i + 1] = sr[i + 1]; dr[i + 2] = sr[i + 2]; }
            dr[i + 3] = 255;
        }
    }
    free(raw);
    *ow = (int)w; *oh = (int)h; *olen = (size_t)w * (size_t)h * 4;
    return out;
}

char *tomoyo_cap_x11_windows_json(int *out_cx, int *out_cy) {
    Display *dpy = XOpenDisplay(NULL);
    if (!dpy) return NULL;
    Window root = DefaultRootWindow(dpy), rw = None, cw = None;
    int rx = 0, ry = 0, wx = 0, wy = 0; unsigned int mask = 0;
    if (XQueryPointer(dpy, root, &rw, &cw, &rx, &ry, &wx, &wy, &mask)) {
        if (out_cx) *out_cx = rx;
        if (out_cy) *out_cy = ry;
    }
    Atom a = XInternAtom(dpy, "_NET_CLIENT_LIST_STACKING", True);
    Atom rt; int rf; unsigned long n = 0, ba = 0; unsigned char *prop = NULL;
    GString *gs = g_string_new("[");
    if (a != None && XGetWindowProperty(dpy, root, a, 0, 256, False, XA_WINDOW, &rt, &rf, &n, &ba, &prop) == Success && prop) {
        Window *ws = (Window *)prop;
        int first = 1;
        for (long i = (long)n - 1; i >= 0; i--) {
            XWindowAttributes wa;
            if (!XGetWindowAttributes(dpy, ws[i], &wa) || wa.map_state != IsViewable || wa.width < 48 || wa.height < 36) continue;
            int tx = 0, ty = 0; Window ch = None;
            XTranslateCoordinates(dpy, ws[i], root, 0, 0, &tx, &ty, &ch);
            g_string_append_printf(gs, "%s{\"x\":%d,\"y\":%d,\"w\":%d,\"h\":%d}", first ? "" : ",", tx, ty, wa.width, wa.height);
            first = 0;
        }
        XFree(prop);
    }
    XCloseDisplay(dpy);
    g_string_append_c(gs, ']');
    char *out = strdup(gs->str);
    g_string_free(gs, TRUE);
    return out;
}

typedef struct {
    GMainLoop *loop;
    const char *suffix;
    uint32_t code;
    GVariant *results;
} portal_wait_t;

static void on_portal_resp(
    GDBusConnection *c, const gchar *s, const gchar *p, const gchar *i,
    const gchar *sig, GVariant *params, gpointer ud
) {
    (void)c; (void)s; (void)i; (void)sig;
    portal_wait_t *pw = (portal_wait_t *)ud;
    if (!p || !pw->suffix || !g_str_has_suffix(p, pw->suffix)) return;
    guint32 rc = 2; GVariant *dict = NULL;
    g_variant_get(params, "(u@a{sv})", &rc, &dict);
    pw->code = rc;
    if (pw->results) g_variant_unref(pw->results);
    pw->results = dict;
    if (pw->loop && g_main_loop_is_running(pw->loop)) g_main_loop_quit(pw->loop);
}

static int portal_step(
    GDBusConnection *bus, const char *method, GVariant *args, const char *tok_suffix, int timeout_ms, GVariant **out_dict, char **out_err
) {
    portal_wait_t pw = { g_main_loop_new(NULL, FALSE), tok_suffix, 99, NULL };
    guint sub = g_dbus_connection_signal_subscribe(
        bus, "org.freedesktop.portal.Desktop", "org.freedesktop.portal.Request", "Response",
        NULL, NULL, G_DBUS_SIGNAL_FLAGS_NONE, on_portal_resp, &pw, NULL);
    GError *err = NULL;
    GVariant *reply = g_dbus_connection_call_sync(
        bus, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.ScreenCast", method, args, G_VARIANT_TYPE("(o)"),
        G_DBUS_CALL_FLAGS_NONE, 4000, NULL, &err);
    if (!reply) {
        if (out_err) *out_err = strdup(err ? err->message : "ScreenCast D-Bus call failed");
        if (err) g_error_free(err);
        g_dbus_connection_signal_unsubscribe(bus, sub);
        g_main_loop_unref(pw.loop);
        return -1;
    }
    g_variant_unref(reply);
    loop_to_t to = { pw.loop, 0 };
    to.tid = g_timeout_add((guint)timeout_ms, on_loop_timeout, &to);
    g_main_loop_run(pw.loop);
    if (to.tid) g_source_remove(to.tid);
    g_dbus_connection_signal_unsubscribe(bus, sub);
    g_main_loop_unref(pw.loop);
    if (pw.code != 0) {
        if (pw.results) g_variant_unref(pw.results);
        if (out_err) *out_err = strdup(pw.code == 1 ? "已取消屏幕共享授权" : "ScreenCast 权限请求超时或被拒绝");
        return -1;
    }
    *out_dict = pw.results;
    return 0;
}

void tomoyo_cap_portal_close_session(void) {
    if (g_sc_bus && g_sc_sess) {
        GVariant *r = g_dbus_connection_call_sync(
            g_sc_bus, "org.freedesktop.portal.Desktop", g_sc_sess,
            "org.freedesktop.portal.Session", "Close",
            NULL, NULL, G_DBUS_CALL_FLAGS_NONE, 500, NULL, NULL);
        if (r) g_variant_unref(r);
    }
    if (g_sc_sess) { free(g_sc_sess); g_sc_sess = NULL; }
}

int tomoyo_cap_portal_screencast(
    int cursor_mode, const char *restore_tok, uint32_t *out_node, int *out_w, int *out_h, char **out_tok, char **out_err
) {
    tomoyo_cap_portal_close_session();
    if (!get_session_bus()) { if (out_err) *out_err = strdup("无法连接 Session D-Bus"); return -1; }
    int pid = (int)getpid();
    char t1[48], s1[48], t2[48], t3[48];
    snprintf(t1, sizeof(t1), "tomoyo_r1_%d", pid); snprintf(s1, sizeof(s1), "tomoyo_s_%d", pid);
    snprintf(t2, sizeof(t2), "tomoyo_r2_%d", pid); snprintf(t3, sizeof(t3), "tomoyo_r3_%d", pid);

    GVariantBuilder b1; g_variant_builder_init(&b1, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&b1, "{sv}", "handle_token", g_variant_new_string(t1));
    g_variant_builder_add(&b1, "{sv}", "session_handle_token", g_variant_new_string(s1));
    GVariant *d1 = NULL;
    if (portal_step(g_sc_bus, "CreateSession", g_variant_new("(a{sv})", &b1), t1, 5000, &d1, out_err) != 0) return -1;
    const gchar *sess_path = NULL;
    if (d1) g_variant_lookup(d1, "session_handle", "&s", &sess_path);
    char *sess = sess_path ? strdup(sess_path) : NULL;
    if (d1) g_variant_unref(d1);
    if (!sess) { if (out_err) *out_err = strdup("ScreenCast 会话创建失败"); return -1; }

    GVariantBuilder b2; g_variant_builder_init(&b2, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&b2, "{sv}", "handle_token", g_variant_new_string(t2));
    g_variant_builder_add(&b2, "{sv}", "types", g_variant_new_uint32(1));
    g_variant_builder_add(&b2, "{sv}", "multiple", g_variant_new_boolean(FALSE));
    g_variant_builder_add(&b2, "{sv}", "cursor_mode", g_variant_new_uint32(cursor_mode ? 2u : 1u));
    g_variant_builder_add(&b2, "{sv}", "persist_mode", g_variant_new_uint32(2));
    if (restore_tok && restore_tok[0]) g_variant_builder_add(&b2, "{sv}", "restore_token", g_variant_new_string(restore_tok));
    GVariant *d2 = NULL;
    if (portal_step(g_sc_bus, "SelectSources", g_variant_new("(oa{sv})", sess, &b2), t2, 5000, &d2, out_err) != 0) { free(sess); return -1; }
    if (d2) g_variant_unref(d2);

    GVariantBuilder b3; g_variant_builder_init(&b3, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&b3, "{sv}", "handle_token", g_variant_new_string(t3));
    GVariant *d3 = NULL;
    if (portal_step(g_sc_bus, "Start", g_variant_new("(osa{sv})", sess, "", &b3), t3, 120000, &d3, out_err) != 0) { free(sess); return -1; }
    if (d3) {
        const gchar *rtok = NULL;
        if (g_variant_lookup(d3, "restore_token", "&s", &rtok) && rtok && out_tok) *out_tok = strdup(rtok);
        GVariant *streams = g_variant_lookup_value(d3, "streams", G_VARIANT_TYPE("a(ua{sv})"));
        if (streams) {
            if (g_variant_n_children(streams) > 0) {
                guint32 nid = 0; GVariant *props = NULL;
                g_variant_get_child(streams, 0, "(u@a{sv})", &nid, &props);
                if (out_node) *out_node = nid;
                if (props) {
                    gint32 sw = 0, sh = 0;
                    if (g_variant_lookup(props, "size", "(ii)", &sw, &sh)) {
                        if (out_w) *out_w = sw;
                        if (out_h) *out_h = sh;
                    }
                    g_variant_unref(props);
                }
            }
            g_variant_unref(streams);
        }
        g_variant_unref(d3);
    }

    GVariantBuilder b4; g_variant_builder_init(&b4, G_VARIANT_TYPE("a{sv}"));
    GUnixFDList *fd_list = NULL; GError *err = NULL;
    GVariant *r4 = g_dbus_connection_call_with_unix_fd_list_sync(
        g_sc_bus, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.ScreenCast", "OpenPipeWireRemote",
        g_variant_new("(oa{sv})", sess, &b4), G_VARIANT_TYPE("(h)"),
        G_DBUS_CALL_FLAGS_NONE, 4000, NULL, &fd_list, NULL, &err);
    if (!r4) {
        free(sess);
        if (out_err) *out_err = strdup(err ? err->message : "OpenPipeWireRemote 失败");
        if (err) g_error_free(err);
        return -1;
    }
    gint32 idx = 0; g_variant_get(r4, "(h)", &idx); g_variant_unref(r4);
    int fd = fd_list ? g_unix_fd_list_get(fd_list, idx, &err) : -1;
    if (fd_list) g_object_unref(fd_list);
    if (fd < 0) {
        free(sess);
        if (out_err) *out_err = strdup(err ? err->message : "获取 PipeWire FD 失败");
    } else {
        g_sc_sess = sess;
    }
    if (err) g_error_free(err);
    return fd;
}
