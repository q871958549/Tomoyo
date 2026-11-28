#include <gio/gio.h>
#include <glib.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef void (*portal_fire_cb)(void *ctx, const char *shortcut_id);
typedef void (*portal_changed_cb)(void *ctx, const char *pairs_utf8);

typedef struct {
    GDBusConnection *bus;
    guint sub_id;
    guint changed_sub_id;
    guint bind_req_sub_id;
    guint list_req_sub_id;
    gchar *session_handle;
    portal_fire_cb cb;
    portal_changed_cb changed_cb;
    void *ctx;
    guint seq;
} portal_hk_t;

typedef struct {
    gboolean done;
    guint32 code;
    gchar *session_handle;
} portal_req_t;

int tomoyo_kga_is_available(void) {
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) {
        if (err) g_error_free(err);
        return 0;
    }
    GVariant *res = g_dbus_connection_call_sync(
        bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        g_variant_new("(s)", "org.kde.kglobalaccel"),
        G_VARIANT_TYPE("(b)"),
        G_DBUS_CALL_FLAGS_NONE,
        250,
        NULL,
        NULL
    );
    int owned = 0;
    if (res) {
        gboolean flag = FALSE;
        g_variant_get(res, "(b)", &flag);
        owned = flag ? 1 : 0;
        g_variant_unref(res);
    }
    g_object_unref(bus);
    return owned;
}

static gchar *portal_sender_segment(GDBusConnection *bus) {
    const gchar *uniq = g_dbus_connection_get_unique_name(bus);
    if (!uniq || !uniq[0]) return g_strdup("unknown");
    gchar *seg = g_strdup(uniq[0] == ':' ? uniq + 1 : uniq);
    for (gchar *p = seg; *p; p++) {
        if (*p == '.') *p = '_';
    }
    return seg;
}

static void emit_shortcuts_array(portal_hk_t *p, GVariant *arr) {
    if (!p || !p->changed_cb || !arr) return;
    GVariantIter it;
    g_variant_iter_init(&it, arr);
    const gchar *sid = NULL;
    GVariant *props = NULL;
    GString *gs = g_string_new("");
    while (g_variant_iter_next(&it, "(&s@a{sv})", &sid, &props)) {
        const gchar *td = NULL;
        if (props && g_variant_lookup(props, "trigger_description", "&s", &td) && sid && sid[0] && td && td[0]) {
            g_string_append(gs, sid);
            g_string_append_c(gs, '=');
            g_string_append(gs, td);
            g_string_append_c(gs, '\n');
        }
        if (props) g_variant_unref(props);
    }
    if (gs->len > 0) {
        p->changed_cb(p->ctx, gs->str);
    }
    g_string_free(gs, TRUE);
}

static void handle_shortcuts_response(portal_hk_t *p, guint *sub_slot, GVariant *params) {
    if (!p || !params) return;
    guint32 code = 2;
    GVariant *dict = NULL;
    g_variant_get(params, "(u@a{sv})", &code, &dict);
    if (code == 0 && dict) {
        GVariant *sc = g_variant_lookup_value(dict, "shortcuts", G_VARIANT_TYPE("a(sa{sv})"));
        if (sc) {
            emit_shortcuts_array(p, sc);
            g_variant_unref(sc);
        }
    }
    if (dict) g_variant_unref(dict);
    if (sub_slot && *sub_slot && p->bus) {
        g_dbus_connection_signal_unsubscribe(p->bus, *sub_slot);
        *sub_slot = 0;
    }
}

static void on_bind_req_response(
    GDBusConnection *c, const gchar *s, const gchar *path, const gchar *iface, const gchar *sig,
    GVariant *params, gpointer ud
) {
    (void)c; (void)s; (void)path; (void)iface; (void)sig;
    portal_hk_t *p = (portal_hk_t *)ud;
    if (p) handle_shortcuts_response(p, &p->bind_req_sub_id, params);
}

static void on_list_req_response(
    GDBusConnection *c, const gchar *s, const gchar *path, const gchar *iface, const gchar *sig,
    GVariant *params, gpointer ud
) {
    (void)c; (void)s; (void)path; (void)iface; (void)sig;
    portal_hk_t *p = (portal_hk_t *)ud;
    if (p) handle_shortcuts_response(p, &p->list_req_sub_id, params);
}

static void on_portal_shortcuts_changed(
    GDBusConnection *c, const gchar *s, const gchar *path, const gchar *iface, const gchar *sig,
    GVariant *params, gpointer ud
) {
    (void)c; (void)s; (void)path; (void)iface; (void)sig;
    portal_hk_t *p = (portal_hk_t *)ud;
    if (!p || !p->session_handle || !params) return;
    const gchar *sh = NULL;
    GVariant *arr = NULL;
    g_variant_get(params, "(&o@a(sa{sv}))", &sh, &arr);
    if (sh && arr && strcmp(sh, p->session_handle) == 0) {
        emit_shortcuts_array(p, arr);
    }
    if (arr) g_variant_unref(arr);
}

static void on_portal_req_response(
    GDBusConnection *c, const gchar *s, const gchar *path, const gchar *iface, const gchar *sig,
    GVariant *params, gpointer ud
) {
    (void)c; (void)s; (void)path; (void)iface; (void)sig;
    portal_req_t *req = (portal_req_t *)ud;
    if (!req || !params) return;
    guint32 code = 2;
    GVariantIter *it = NULL;
    g_variant_get(params, "(ua{sv})", &code, &it);
    req->code = code;
    const gchar *k = NULL;
    GVariant *v = NULL;
    while (it && g_variant_iter_next(it, "{&sv}", &k, &v)) {
        if (k && strcmp(k, "session_handle") == 0 && !req->session_handle) {
            if (g_variant_is_of_type(v, G_VARIANT_TYPE_STRING) ||
                g_variant_is_of_type(v, G_VARIANT_TYPE_OBJECT_PATH)) {
                req->session_handle = g_strdup(g_variant_get_string(v, NULL));
            }
        }
        g_variant_unref(v);
    }
    if (it) g_variant_iter_free(it);
    req->done = TRUE;
}

static void on_portal_activated(
    GDBusConnection *c, const gchar *s, const gchar *path, const gchar *iface, const gchar *sig,
    GVariant *params, gpointer ud
) {
    (void)c; (void)s; (void)path; (void)iface; (void)sig;
    portal_hk_t *p = (portal_hk_t *)ud;
    if (!p || !p->cb || !p->session_handle || !params) return;
    const gchar *sh = NULL;
    const gchar *sid = NULL;
    guint64 ts = 0;
    GVariant *opts = NULL;
    g_variant_get(params, "(&o&st@a{sv})", &sh, &sid, &ts, &opts);
    if (opts) g_variant_unref(opts);
    if (sh && sid && strcmp(sh, p->session_handle) == 0) {
        p->cb(p->ctx, sid);
    }
}

static guint32 query_portal_version(GDBusConnection *bus) {
    GVariant *res = g_dbus_connection_call_sync(
        bus, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
        "org.freedesktop.DBus.Properties", "Get",
        g_variant_new("(ss)", "org.freedesktop.portal.GlobalShortcuts", "version"),
        G_VARIANT_TYPE("(v)"), G_DBUS_CALL_FLAGS_NONE, 350, NULL, NULL
    );
    if (!res) return 0;
    GVariant *inner = NULL;
    guint32 ver = 0;
    g_variant_get(res, "(v)", &inner);
    if (inner && g_variant_is_of_type(inner, G_VARIANT_TYPE_UINT32)) {
        ver = g_variant_get_uint32(inner);
    }
    if (inner) g_variant_unref(inner);
    g_variant_unref(res);
    return ver;
}

void *tomoyo_portal_hk_connect(portal_fire_cb cb, portal_changed_cb changed_cb, void *ctx) {
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) {
        if (err) g_error_free(err);
        return NULL;
    }
    if (query_portal_version(bus) < 1) {
        g_object_unref(bus);
        return NULL;
    }

    unsigned long pid = (unsigned long)getpid();
    gchar *tok = g_strdup_printf("tomoyo_hk_%lu", pid);
    gchar *stok = g_strdup_printf("tomoyo_sess_%lu", pid);
    gchar *sender = portal_sender_segment(bus);
    gchar *req_path = g_strdup_printf("/org/freedesktop/portal/desktop/request/%s/%s", sender, tok);
    g_free(sender);

    portal_req_t req = { FALSE, 2, NULL };
    guint rsub = g_dbus_connection_signal_subscribe(
        bus, "org.freedesktop.portal.Desktop", "org.freedesktop.portal.Request", "Response",
        req_path, NULL, G_DBUS_SIGNAL_FLAGS_NONE, on_portal_req_response, &req, NULL
    );

    GVariantBuilder opts;
    g_variant_builder_init(&opts, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&opts, "{sv}", "handle_token", g_variant_new_string(tok));
    g_variant_builder_add(&opts, "{sv}", "session_handle_token", g_variant_new_string(stok));

    GVariant *call_res = g_dbus_connection_call_sync(
        bus, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.GlobalShortcuts", "CreateSession",
        g_variant_new("(a{sv})", &opts), G_VARIANT_TYPE("(o)"),
        G_DBUS_CALL_FLAGS_NONE, 400, NULL, NULL
    );
    if (call_res) {
        const gchar *returned_path = NULL;
        g_variant_get(call_res, "(&o)", &returned_path);
        if (returned_path && strcmp(returned_path, req_path) != 0) {
            g_dbus_connection_signal_unsubscribe(bus, rsub);
            rsub = g_dbus_connection_signal_subscribe(
                bus, "org.freedesktop.portal.Desktop", "org.freedesktop.portal.Request", "Response",
                returned_path, NULL, G_DBUS_SIGNAL_FLAGS_NONE, on_portal_req_response, &req, NULL
            );
        }
        g_variant_unref(call_res);
        for (int i = 0; i < 300 && !req.done; i++) {
            while (g_main_context_iteration(NULL, FALSE)) {}
            if (!req.done) g_usleep(5000);
        }
    }
    g_dbus_connection_signal_unsubscribe(bus, rsub);
    g_free(tok);
    g_free(stok);
    g_free(req_path);

    if (!req.done || req.code != 0 || !req.session_handle || !req.session_handle[0]) {
        g_free(req.session_handle);
        g_object_unref(bus);
        return NULL;
    }

    portal_hk_t *p = (portal_hk_t *)calloc(1, sizeof(portal_hk_t));
    p->bus = bus;
    p->session_handle = req.session_handle;
    p->cb = cb;
    p->changed_cb = changed_cb;
    p->ctx = ctx;
    p->sub_id = g_dbus_connection_signal_subscribe(
        bus, "org.freedesktop.portal.Desktop", "org.freedesktop.portal.GlobalShortcuts", "Activated",
        "/org/freedesktop/portal/desktop", NULL, G_DBUS_SIGNAL_FLAGS_NONE, on_portal_activated, p, NULL
    );
    p->changed_sub_id = g_dbus_connection_signal_subscribe(
        bus, "org.freedesktop.portal.Desktop", "org.freedesktop.portal.GlobalShortcuts", "ShortcutsChanged",
        "/org/freedesktop/portal/desktop", NULL, G_DBUS_SIGNAL_FLAGS_NONE, on_portal_shortcuts_changed, p, NULL
    );
    return p;
}

static void subscribe_request_slot(portal_hk_t *p, guint *slot, const gchar *tok, GDBusSignalCallback cb) {
    if (*slot) {
        g_dbus_connection_signal_unsubscribe(p->bus, *slot);
        *slot = 0;
    }
    gchar *sender = portal_sender_segment(p->bus);
    gchar *req_path = g_strdup_printf("/org/freedesktop/portal/desktop/request/%s/%s", sender, tok);
    g_free(sender);
    *slot = g_dbus_connection_signal_subscribe(
        p->bus, "org.freedesktop.portal.Desktop", "org.freedesktop.portal.Request", "Response",
        req_path, NULL, G_DBUS_SIGNAL_FLAGS_NONE, cb, p, NULL
    );
    g_free(req_path);
}

int tomoyo_portal_hk_bind(
    void *phk, const char *const *ids, const char *const *descs, const char *const *triggers, size_t count
) {
    portal_hk_t *p = (portal_hk_t *)phk;
    if (!p || !p->bus || !p->session_handle || !ids || !descs || !triggers || count == 0) {
        return 0;
    }
    GVariantBuilder list;
    g_variant_builder_init(&list, G_VARIANT_TYPE("a(sa{sv})"));
    for (size_t i = 0; i < count; i++) {
        GVariantBuilder props;
        g_variant_builder_init(&props, G_VARIANT_TYPE("a{sv}"));
        g_variant_builder_add(&props, "{sv}", "description", g_variant_new_string(descs[i] ? descs[i] : ids[i]));
        if (triggers[i] && triggers[i][0]) {
            g_variant_builder_add(&props, "{sv}", "preferred_trigger", g_variant_new_string(triggers[i]));
        }
        g_variant_builder_add(&list, "(sa{sv})", ids[i], &props);
    }
    p->seq++;
    gchar *tok = g_strdup_printf("tomoyo_bind_%lu_%u", (unsigned long)getpid(), p->seq);
    subscribe_request_slot(p, &p->bind_req_sub_id, tok, on_bind_req_response);
    GVariantBuilder opts;
    g_variant_builder_init(&opts, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&opts, "{sv}", "handle_token", g_variant_new_string(tok));
    g_free(tok);

    g_dbus_connection_call(
        p->bus, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.GlobalShortcuts", "BindShortcuts",
        g_variant_new("(oa(sa{sv})sa{sv})", p->session_handle, &list, "", &opts),
        G_VARIANT_TYPE("(o)"), G_DBUS_CALL_FLAGS_NONE, 5000, NULL, NULL, NULL
    );
    return 1;
}

int tomoyo_portal_hk_list(void *phk) {
    portal_hk_t *p = (portal_hk_t *)phk;
    if (!p || !p->bus || !p->session_handle) return 0;
    p->seq++;
    gchar *tok = g_strdup_printf("tomoyo_list_%lu_%u", (unsigned long)getpid(), p->seq);
    subscribe_request_slot(p, &p->list_req_sub_id, tok, on_list_req_response);
    GVariantBuilder opts;
    g_variant_builder_init(&opts, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&opts, "{sv}", "handle_token", g_variant_new_string(tok));
    g_free(tok);

    g_dbus_connection_call(
        p->bus, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.GlobalShortcuts", "ListShortcuts",
        g_variant_new("(oa{sv})", p->session_handle, &opts),
        G_VARIANT_TYPE("(o)"), G_DBUS_CALL_FLAGS_NONE, 2000, NULL, NULL, NULL
    );
    return 1;
}

void tomoyo_portal_hk_destroy(void *phk) {
    portal_hk_t *p = (portal_hk_t *)phk;
    if (!p) return;
    if (p->bus && p->sub_id) g_dbus_connection_signal_unsubscribe(p->bus, p->sub_id);
    if (p->bus && p->changed_sub_id) g_dbus_connection_signal_unsubscribe(p->bus, p->changed_sub_id);
    if (p->bus && p->bind_req_sub_id) g_dbus_connection_signal_unsubscribe(p->bus, p->bind_req_sub_id);
    if (p->bus && p->list_req_sub_id) g_dbus_connection_signal_unsubscribe(p->bus, p->list_req_sub_id);
    if (p->bus && p->session_handle) {
        GVariant *r = g_dbus_connection_call_sync(
            p->bus, "org.freedesktop.portal.Desktop", p->session_handle,
            "org.freedesktop.portal.Session", "Close", NULL, NULL,
            G_DBUS_CALL_FLAGS_NONE, 200, NULL, NULL
        );
        if (r) g_variant_unref(r);
    }
    g_free(p->session_handle);
    if (p->bus) g_object_unref(p->bus);
    free(p);
}
