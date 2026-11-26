#include <gdk/gdkwayland.h>
#include <gio/gio.h>
#include <gtk/gtk.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

typedef void (*tomoyo_pos_report_cb)(void *ctx, int x, int y, int w, int h);

typedef struct {
    GDBusConnection *bus;
    guint reg_id;
    char script_path[512];
    tomoyo_pos_report_cb cb;
    void *ctx;
} kwin_pos_t;

static const char *KWIN_OBJ_PATH = "/dev/tomoyo/WindowPos";
static const char *KWIN_IFACE = "dev.tomoyo.WindowPos";
static const char *KWIN_SCRIPT_NAME = "tomoyo_pos_bridge";
static const char *INTROSPECT_XML =
    "<node>"
    "  <interface name='dev.tomoyo.WindowPos'>"
    "    <method name='Report'>"
    "      <arg type='s' name='payload' direction='in'/>"
    "    </method>"
    "  </interface>"
    "</node>";

static void on_dbus_call(
    GDBusConnection *c, const gchar *sender, const gchar *path, const gchar *iface,
    const gchar *method, GVariant *params, GDBusMethodInvocation *inv, gpointer ud
) {
    (void)c; (void)sender; (void)path; (void)iface;
    kwin_pos_t *kp = (kwin_pos_t *)ud;
    if (kp && strcmp(method, "Report") == 0) {
        const gchar *payload = NULL;
        g_variant_get(params, "(&s)", &payload);
        if (payload && kp->cb) {
            int x = 0, y = 0, w = 0, h = 0;
            if (sscanf(payload, "%d,%d,%d,%d", &x, &y, &w, &h) == 4) {
                kp->cb(kp->ctx, x, y, w, h);
            }
        }
    }
    g_dbus_method_invocation_return_value(inv, NULL);
}

static const GDBusInterfaceVTable KWIN_VTABLE = { on_dbus_call, NULL, NULL, {0} };
static int g_script_epoch = 0;

static void unload_kwin_script(GDBusConnection *bus) {
    GVariant *r = g_dbus_connection_call_sync(
        bus, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting",
        "unloadScript", g_variant_new("(s)", KWIN_SCRIPT_NAME),
        G_VARIANT_TYPE("(b)"), G_DBUS_CALL_FLAGS_NONE, 250, NULL, NULL);
    if (r) g_variant_unref(r);
}

static int load_and_run_kwin_script(kwin_pos_t *kp, int has_pos, int init_x, int init_y) {
    if (!kp || !kp->bus || !kp->script_path[0]) return 0;
    const gchar *uname = g_dbus_connection_get_unique_name(kp->bus);
    if (!uname || !uname[0]) return 0;

    char xs[32], ys[32];
    if (has_pos) {
        snprintf(xs, sizeof(xs), "%d", init_x);
        snprintf(ys, sizeof(ys), "%d", init_y);
    } else {
        snprintf(xs, sizeof(xs), "null");
        snprintf(ys, sizeof(ys), "null");
    }

    FILE *f = fopen(kp->script_path, "w");
    if (!f) return 0;
    int epoch = ++g_script_epoch;
    fprintf(f,
        "var BUS = \"%s\";\n"
        "var EPOCH = %d;\n"
        "var targetX = %s, targetY = %s;\n"
        "function isTomoyo(w) {\n"
        "  if (!w || w.modal || w.popupWindow || w.dock || w.desktopWindow || w.transient) return false;\n"
        "  var rc = String(w.resourceClass || \"\").toLowerCase();\n"
        "  var rn = String(w.resourceName || \"\").toLowerCase();\n"
        "  var df = String(w.desktopFileName || \"\").toLowerCase();\n"
        "  if (rc !== \"tomoyo\" && rn !== \"tomoyo\" && df !== \"tomoyo\") return false;\n"
        "  return String(w.caption || \"\") === \"Tomoyo\";\n"
        "}\n"
        "function applyPos(w) {\n"
        "  var g = w.frameGeometry;\n"
        "  if (!g || g.width <= 0 || g.height <= 0) return false;\n"
        "  var cx, cy;\n"
        "  if (targetX !== null && targetY !== null) {\n"
        "    var vs = workspace.virtualScreenGeometry;\n"
        "    cx = Math.round(targetX); cy = Math.round(targetY);\n"
        "    if (vs && vs.width > g.width) {\n"
        "      if (cx < vs.x) cx = vs.x;\n"
        "      if (cx > vs.x + vs.width - g.width) cx = Math.round(vs.x + vs.width - g.width);\n"
        "    }\n"
        "    if (vs && vs.height > g.height) {\n"
        "      if (cy < vs.y) cy = vs.y;\n"
        "      if (cy > vs.y + vs.height - g.height) cy = Math.round(vs.y + vs.height - g.height);\n"
        "    }\n"
        "  } else {\n"
        "    var ca = workspace.clientArea(KWin.PlacementArea, workspace.activeScreen, workspace.currentDesktop);\n"
        "    if (!ca || ca.width <= 0) return false;\n"
        "    cx = Math.round(ca.x + (ca.width - g.width) / 2);\n"
        "    cy = Math.round(ca.y + Math.max(40, Math.round(ca.height / 5)));\n"
        "  }\n"
        "  if (Math.abs(g.x - cx) > 1 || Math.abs(g.y - cy) > 1) {\n"
        "    var ng = w.frameGeometry; ng.x = cx; ng.y = cy; w.frameGeometry = ng;\n"
        "  }\n"
        "  return true;\n"
        "}\n"
        "function track(w) {\n"
        "  if (!isTomoyo(w)) return;\n"
        "  w.keepAbove = true; w.skipTaskbar = true; w.skipPager = true;\n"
        "  var placed = applyPos(w);\n"
        "  if (w._tomoyoEpoch === EPOCH) return;\n"
        "  w._tomoyoEpoch = EPOCH;\n"
        "  w.frameGeometryChanged.connect(function() {\n"
        "    if (!placed) placed = applyPos(w);\n"
        "  });\n"
        "  w.interactiveMoveResizeFinished.connect(function() {\n"
        "    var g = w.frameGeometry; if (!g) return;\n"
        "    targetX = Math.round(g.x); targetY = Math.round(g.y);\n"
        "    callDBus(BUS, \"%s\", \"%s\", \"Report\", \"\" + targetX + \",\" + targetY + \",\" + Math.round(g.width) + \",\" + Math.round(g.height));\n"
        "  });\n"
        "}\n"
        "workspace.windowAdded.connect(track);\n"
        "workspace.windowList().forEach(track);\n",
        uname, epoch, xs, ys, KWIN_OBJ_PATH, KWIN_IFACE);
    fclose(f);
    chmod(kp->script_path, 0600);

    unload_kwin_script(kp->bus);
    GError *err = NULL;
    GVariant *lr = g_dbus_connection_call_sync(
        kp->bus, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting",
        "loadScript", g_variant_new("(ss)", kp->script_path, KWIN_SCRIPT_NAME),
        G_VARIANT_TYPE("(i)"), G_DBUS_CALL_FLAGS_NONE, 300, NULL, &err);
    if (!lr) { if (err) g_error_free(err); return 0; }
    gint32 sid = -1;
    g_variant_get(lr, "(i)", &sid);
    g_variant_unref(lr);
    if (sid < 0) return 0;

    char obj[64];
    snprintf(obj, sizeof(obj), "/Scripting/Script%d", (int)sid);
    GVariant *rr = g_dbus_connection_call_sync(
        kp->bus, "org.kde.KWin", obj, "org.kde.kwin.Script",
        "run", NULL, NULL, G_DBUS_CALL_FLAGS_NONE, 300, NULL, NULL);
    if (rr) { g_variant_unref(rr); return 1; }
    return 0;
}

void *tomoyo_kwin_pos_start(int has_pos, int init_x, int init_y, tomoyo_pos_report_cb cb, void *ctx) {
    GdkDisplay *gd = gdk_display_get_default();
    if (!gd || !GDK_IS_WAYLAND_DISPLAY(gd)) return NULL;
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) { if (err) g_error_free(err); return NULL; }
    GDBusNodeInfo *node = g_dbus_node_info_new_for_xml(INTROSPECT_XML, NULL);
    if (!node || !node->interfaces || !node->interfaces[0]) {
        if (node) g_dbus_node_info_unref(node);
        g_object_unref(bus);
        return NULL;
    }
    kwin_pos_t *kp = (kwin_pos_t *)calloc(1, sizeof(kwin_pos_t));
    if (!kp) { g_dbus_node_info_unref(node); g_object_unref(bus); return NULL; }
    kp->bus = bus;
    kp->cb = cb;
    kp->ctx = ctx;
    const char *rdir = g_get_user_runtime_dir();
    snprintf(kp->script_path, sizeof(kp->script_path), "%s/tomoyo-kwin-pos.js", (rdir && rdir[0]) ? rdir : "/tmp");

    kp->reg_id = g_dbus_connection_register_object(
        bus, KWIN_OBJ_PATH, node->interfaces[0], &KWIN_VTABLE, kp, NULL, NULL);
    g_dbus_node_info_unref(node);
    if (!kp->reg_id) {
        g_object_unref(bus);
        free(kp);
        return NULL;
    }
    load_and_run_kwin_script(kp, has_pos, init_x, init_y);
    return kp;
}

void tomoyo_kwin_pos_update_target(void *handle, int has_pos, int x, int y) {
    kwin_pos_t *kp = (kwin_pos_t *)handle;
    if (kp) load_and_run_kwin_script(kp, has_pos, x, y);
}

void tomoyo_kwin_pos_stop(void *handle) {
    kwin_pos_t *kp = (kwin_pos_t *)handle;
    if (!kp) return;
    if (kp->bus) {
        unload_kwin_script(kp->bus);
        if (kp->reg_id) g_dbus_connection_unregister_object(kp->bus, kp->reg_id);
        g_object_unref(kp->bus);
    }
    if (kp->script_path[0]) unlink(kp->script_path);
    free(kp);
}
