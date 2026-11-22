#include <gio/gio.h>
#include <stdio.h>
#include <string.h>

int tomoyo_kga_query_owner(int qt_key, char *comp_out, size_t comp_len, char *friendly_out, size_t fr_len) {
    if (comp_out && comp_len > 0) comp_out[0] = '\0';
    if (friendly_out && fr_len > 0) friendly_out[0] = '\0';
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) {
        if (err) g_error_free(err);
        return -1;
    }
    GVariant *res = g_dbus_connection_call_sync(
        bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel",
        "action", g_variant_new("(i)", qt_key), G_VARIANT_TYPE("(as)"),
        G_DBUS_CALL_FLAGS_NONE, 250, NULL, &err
    );
    if (!res) {
        if (err) g_error_free(err);
        g_object_unref(bus);
        return -1;
    }
    GVariantIter *iter = NULL;
    g_variant_get(res, "(as)", &iter);
    const gchar *parts[4] = { NULL, NULL, NULL, NULL };
    int idx = 0;
    const gchar *s = NULL;
    while (idx < 4 && g_variant_iter_next(iter, "&s", &s)) {
        parts[idx++] = s;
    }
    int owned = 0;
    if (idx >= 1 && parts[0] && parts[0][0] != '\0') {
        owned = 1;
        if (comp_out && comp_len > 0) snprintf(comp_out, comp_len, "%s", parts[0]);
        const char *fr = (idx >= 3 && parts[2] && parts[2][0]) ? parts[2] : parts[0];
        if (friendly_out && fr_len > 0) snprintf(friendly_out, fr_len, "%s", fr);
    }
    g_variant_iter_free(iter);
    g_variant_unref(res);
    g_object_unref(bus);
    return owned;
}

void tomoyo_kga_block_global(int block) {
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!bus) {
        if (err) g_error_free(err);
        return;
    }
    GVariant *res = g_dbus_connection_call_sync(
        bus, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel",
        "blockGlobalShortcuts", g_variant_new("(b)", block ? TRUE : FALSE),
        NULL, G_DBUS_CALL_FLAGS_NONE, 250, NULL, &err
    );
    if (err) g_error_free(err);
    if (res) g_variant_unref(res);
    g_object_unref(bus);
}
