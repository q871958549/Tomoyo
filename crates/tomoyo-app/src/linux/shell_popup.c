#include "shell_internal.h"
#include <stdlib.h>
#include <string.h>

typedef struct {
    tomoyo_cmd_cb cb;
    void *ctx;
    uint32_t cmd;
    tomoyo_app_t *app;
} menu_pick_t;

static void on_popup_activate(GtkMenuItem *it, gpointer ud) {
    (void)it;
    menu_pick_t *p = (menu_pick_t *)ud;
    if (p && p->app) p->app->menu_picked = 1;
    if (p && p->cb) p->cb(p->ctx, p->cmd);
}

static void free_menu_pick(gpointer ud, GClosure *cl) {
    (void)cl;
    free(ud);
}

static gboolean destroy_menu_idle(gpointer ud) {
    gtk_widget_destroy(GTK_WIDGET(ud));
    g_object_unref(ud);
    return G_SOURCE_REMOVE;
}

static void on_popup_done(GtkMenuShell *ms, gpointer ud) {
    menu_pick_t *p = (menu_pick_t *)ud;
    uint32_t code = (p && p->app && p->app->menu_picked) ? 0xFFFFFFFEu : 0u;
    if (p && p->cb) p->cb(p->ctx, code);
    g_idle_add(destroy_menu_idle, ms);
}

void tomoyo_linux_tray_setup(tomoyo_app_t *a, const char *icon_path, const char *hotkey_lbl) {
    if (!a || !icon_path) return;
    G_GNUC_BEGIN_IGNORE_DEPRECATIONS
    a->tray = app_indicator_new("tomoyo-launcher", icon_path, APP_INDICATOR_CATEGORY_APPLICATION_STATUS);
    G_GNUC_END_IGNORE_DEPRECATIONS
    app_indicator_set_status(a->tray, APP_INDICATOR_STATUS_ACTIVE);
    app_indicator_set_title(a->tray, hotkey_lbl ? hotkey_lbl : "Tomoyo");
    a->tray_menu = gtk_menu_new();
    const struct { const char *lbl; uint32_t cmd; } items[] = {
        { "显示 Tomoyo", 1 }, { "设置中心...", 2 }, { "剪贴板历史", 3 },
        { "多源翻译", 4 }, { "区域截图", 5 }, { "区域录屏", 6 }, { NULL, 0 }, { "退出 Tomoyo", 9 }
    };
    for (size_t i = 0; i < sizeof(items) / sizeof(items[0]); i++) {
        if (!items[i].lbl) {
            gtk_menu_shell_append(GTK_MENU_SHELL(a->tray_menu), gtk_separator_menu_item_new());
            continue;
        }
        GtkWidget *mi = gtk_menu_item_new_with_label(items[i].lbl);
        menu_pick_t *p = (menu_pick_t *)malloc(sizeof(menu_pick_t));
        p->cb = a->on_tray_cmd; p->ctx = a->ctx; p->cmd = items[i].cmd; p->app = NULL;
        g_signal_connect_data(mi, "activate", G_CALLBACK(on_popup_activate), p, free_menu_pick, 0);
        gtk_menu_shell_append(GTK_MENU_SHELL(a->tray_menu), mi);
    }
    gtk_widget_show_all(a->tray_menu);
    app_indicator_set_menu(a->tray, GTK_MENU(a->tray_menu));
}

void tomoyo_linux_show_popup(
    tomoyo_app_t *a,
    const tomoyo_menu_item_t *items,
    size_t n,
    tomoyo_cmd_cb cb,
    void *ctx
) {
    if (a) a->menu_picked = 0;
    GtkWidget *root = (GtkWidget *)g_object_ref_sink(gtk_menu_new());
    GtkWidget **subs = (GtkWidget **)calloc(n > 0 ? n : 1, sizeof(GtkWidget *));
    for (size_t i = 0; i < n && subs; i++) {
        GtkWidget *pm = (items[i].parent_idx >= 0 && (size_t)items[i].parent_idx < n && subs[items[i].parent_idx])
            ? subs[items[i].parent_idx] : root;
        if (!items[i].label || strcmp(items[i].label, "---") == 0) {
            gtk_menu_shell_append(GTK_MENU_SHELL(pm), gtk_separator_menu_item_new());
            continue;
        }
        gboolean is_sub = (items[i].cmd == 0);
        GtkWidget *mi = (!is_sub && items[i].checked)
            ? gtk_check_menu_item_new_with_label(items[i].label)
            : gtk_menu_item_new_with_label(items[i].label);
        if (!is_sub && items[i].checked) gtk_check_menu_item_set_active(GTK_CHECK_MENU_ITEM(mi), TRUE);
        if (items[i].disabled) gtk_widget_set_sensitive(mi, FALSE);
        if (is_sub) {
            subs[i] = gtk_menu_new();
            gtk_menu_item_set_submenu(GTK_MENU_ITEM(mi), subs[i]);
        } else {
            menu_pick_t *p = (menu_pick_t *)malloc(sizeof(menu_pick_t));
            p->cb = cb; p->ctx = ctx; p->cmd = items[i].cmd; p->app = a;
            g_signal_connect_data(mi, "activate", G_CALLBACK(on_popup_activate), p, free_menu_pick, 0);
        }
        gtk_menu_shell_append(GTK_MENU_SHELL(pm), mi);
    }
    free(subs);
    menu_pick_t *pd = (menu_pick_t *)malloc(sizeof(menu_pick_t));
    pd->cb = cb; pd->ctx = ctx; pd->cmd = 0; pd->app = a;
    g_signal_connect_data(root, "selection-done", G_CALLBACK(on_popup_done), pd, free_menu_pick, 0);
    gtk_widget_show_all(root);
    GdkEvent *ev = gtk_get_current_event();
    if (a && a->popup_from_kb && a->entry) {
        gtk_menu_popup_at_widget(GTK_MENU(root), a->entry, GDK_GRAVITY_SOUTH_WEST, GDK_GRAVITY_NORTH_WEST, ev);
    } else {
        gtk_menu_popup_at_pointer(GTK_MENU(root), ev);
    }
    if (ev) gdk_event_free(ev);
    if (a) a->popup_from_kb = 0;
}
