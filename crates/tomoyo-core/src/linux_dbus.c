#include <stdint.h>
#include <stddef.h>
#include <string.h>

#ifdef HAVE_GIO_2_0
#include <gio/gio.h>
#endif

typedef void (*tomoyo_win_cb)(void *ctx, const char *id, const char *title, const char *app_id);
typedef void (*tomoyo_str_cb)(void *ctx, const char *s, size_t len);

int tomoyo_dbus_krunner_windows(tomoyo_win_cb cb, void *ctx) {
#ifdef HAVE_GIO_2_0
    if (!cb) return 0;
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) {
        if (err) g_error_free(err);
        return 0;
    }
    GVariant *res = g_dbus_connection_call_sync(
        bus,
        "org.kde.KWin",
        "/WindowsRunner",
        "org.kde.krunner1",
        "Match",
        g_variant_new("(s)", ""),
        G_VARIANT_TYPE("(a(sssuda{sv}))"),
        G_DBUS_CALL_FLAGS_NONE,
        400,
        NULL,
        &err
    );
    if (!res) {
        if (err) g_error_free(err);
        g_object_unref(bus);
        return 0;
    }
    GVariantIter *iter = NULL;
    g_variant_get(res, "(a(sssuda{sv}))", &iter);
    const gchar *id = NULL;
    const gchar *title = NULL;
    const gchar *icon = NULL;
    guint32 typ = 0;
    gdouble rel = 0.0;
    GVariant *props = NULL;
    while (g_variant_iter_next(iter, "(&s&s&sud@a{sv})", &id, &title, &icon, &typ, &rel, &props)) {
        if (id && title && title[0] != '\0') {
            cb(ctx, id, title, icon ? icon : "");
        }
        if (props) g_variant_unref(props);
    }
    g_variant_iter_free(iter);
    g_variant_unref(res);
    g_object_unref(bus);
    return 1;
#else
    (void)cb; (void)ctx;
    return 0;
#endif
}

int tomoyo_dbus_krunner_run(const char *match_id) {
#ifdef HAVE_GIO_2_0
    if (!match_id || !match_id[0]) return 0;
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) {
        if (err) g_error_free(err);
        return 0;
    }
    GVariant *res = g_dbus_connection_call_sync(
        bus,
        "org.kde.KWin",
        "/WindowsRunner",
        "org.kde.krunner1",
        "Run",
        g_variant_new("(ss)", match_id, ""),
        NULL,
        G_DBUS_CALL_FLAGS_NONE,
        400,
        NULL,
        &err
    );
    if (err) g_error_free(err);
    if (res) g_variant_unref(res);
    g_object_unref(bus);
    return res != NULL ? 1 : 0;
#else
    (void)match_id;
    return 0;
#endif
}

int tomoyo_dbus_reveal_file(const char *file_uri) {
#ifdef HAVE_GIO_2_0
    if (!file_uri || !file_uri[0]) return 0;
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) {
        if (err) g_error_free(err);
        return 0;
    }
    const gchar *uris[2] = { file_uri, NULL };
    GVariant *res = g_dbus_connection_call_sync(
        bus,
        "org.freedesktop.FileManager1",
        "/org/freedesktop/FileManager1",
        "org.freedesktop.FileManager1",
        "ShowItems",
        g_variant_new("(^ass)", uris, ""),
        NULL,
        G_DBUS_CALL_FLAGS_NONE,
        1000,
        NULL,
        &err
    );
    if (err) g_error_free(err);
    if (res) g_variant_unref(res);
    g_object_unref(bus);
    return res != NULL ? 1 : 0;
#else
    (void)file_uri;
    return 0;
#endif
}

int tomoyo_dbus_klipper_get(tomoyo_str_cb cb, void *ctx) {
#ifdef HAVE_GIO_2_0
    if (!cb) return 0;
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) {
        if (err) g_error_free(err);
        return 0;
    }
    GVariant *res = g_dbus_connection_call_sync(
        bus,
        "org.kde.klipper",
        "/klipper",
        "org.kde.klipper.klipper",
        "getClipboardContents",
        NULL,
        G_VARIANT_TYPE("(s)"),
        G_DBUS_CALL_FLAGS_NONE,
        300,
        NULL,
        &err
    );
    if (!res) {
        if (err) g_error_free(err);
        g_object_unref(bus);
        return 0;
    }
    const gchar *txt = NULL;
    g_variant_get(res, "(&s)", &txt);
    if (txt) cb(ctx, txt, strlen(txt));
    g_variant_unref(res);
    g_object_unref(bus);
    return txt != NULL ? 1 : 0;
#else
    (void)cb; (void)ctx;
    return 0;
#endif
}
