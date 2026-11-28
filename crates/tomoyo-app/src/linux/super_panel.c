#include <gtk/gtk.h>
#ifdef GDK_WINDOWING_WAYLAND
#include <gdk/gdkwayland.h>
#endif
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef void (*tomoyo_sp_pick_cb)(void *ctx, int idx);

static GtkWidget *g_sp_win = NULL;
static GtkCssProvider *g_sp_css = NULL;
static tomoyo_sp_pick_cb g_sp_cb = NULL;
static void *g_sp_ctx = NULL;
static size_t g_sp_count = 0;
static gint64 g_sp_open_us = 0;

void tomoyo_linux_super_panel_close(void) {
    if (g_sp_win) {
        GtkWidget *w = g_sp_win;
        g_sp_win = NULL;
        g_sp_cb = NULL;
        g_sp_ctx = NULL;
        g_sp_count = 0;
        gtk_widget_destroy(w);
    }
}

static void dispatch_pick(int idx) {
    tomoyo_sp_pick_cb cb = g_sp_cb;
    void *ctx = g_sp_ctx;
    tomoyo_linux_super_panel_close();
    if (cb) cb(ctx, idx);
}

static void on_btn_clicked(GtkButton *b, gpointer ud) {
    (void)b;
    dispatch_pick(GPOINTER_TO_INT(ud));
}

static gboolean on_sp_key(GtkWidget *w, GdkEventKey *ev, gpointer ud) {
    (void)w; (void)ud;
    if (ev->keyval == GDK_KEY_Escape) {
        tomoyo_linux_super_panel_close();
        return TRUE;
    }
    if (ev->keyval >= GDK_KEY_1 && ev->keyval <= GDK_KEY_6) {
        size_t idx = (size_t)(ev->keyval - GDK_KEY_1);
        if (idx < g_sp_count) {
            dispatch_pick((int)idx);
            return TRUE;
        }
    }
    return FALSE;
}

static gboolean on_sp_focus_out(GtkWidget *w, GdkEventFocus *ev, gpointer ud) {
    (void)w; (void)ev; (void)ud;
    if (g_get_monotonic_time() - g_sp_open_us > 120000) {
        tomoyo_linux_super_panel_close();
    }
    return FALSE;
}

static void apply_sp_css(
    GtkWidget *win, uint32_t bg_rgb, uint32_t fg_rgb, uint32_t muted_rgb,
    uint32_t border_rgb, uint32_t accent_rgb, const char *font_css
) {
    if (!g_sp_css) g_sp_css = gtk_css_provider_new();
    const char *fam = (font_css && font_css[0]) ? font_css : "sans-serif";
    gchar *css = g_strdup_printf(
        "window#tomoyo-sp { background-color: #%06x; border: 1px solid #%06x; border-radius: 10px; } "
        "label#tomoyo-sp-hdr { color: #%06x; font-family: %s; font-size: 12px; padding: 6px 10px 4px 10px; } "
        "button.tomoyo-sp-btn { background: transparent; background-image: none; color: #%06x; "
        "font-family: %s; font-size: 13px; border: none; border-radius: 6px; padding: 6px 10px; box-shadow: none; } "
        "button.tomoyo-sp-btn:hover, button.tomoyo-sp-btn:focus { background-color: rgba(%u,%u,%u,0.16); color: #%06x; }",
        bg_rgb & 0xffffff, border_rgb & 0xffffff,
        muted_rgb & 0xffffff, fam,
        fg_rgb & 0xffffff, fam,
        (accent_rgb >> 16) & 0xff, (accent_rgb >> 8) & 0xff, accent_rgb & 0xff, fg_rgb & 0xffffff);
    if (css) {
        gtk_css_provider_load_from_data(g_sp_css, css, -1, NULL);
        g_free(css);
    }
    gtk_style_context_add_provider_for_screen(
        gtk_widget_get_screen(win), GTK_STYLE_PROVIDER(g_sp_css), GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
}

static void position_near_pointer(GtkWindow *win, int n_items) {
#ifdef GDK_WINDOWING_WAYLAND
    GdkDisplay *gd = gdk_display_get_default();
    if (gd && GDK_IS_WAYLAND_DISPLAY(gd)) return;
#endif
    GdkDisplay *d = gdk_display_get_default();
    GdkSeat *seat = d ? gdk_display_get_default_seat(d) : NULL;
    GdkDevice *ptr = seat ? gdk_seat_get_pointer(seat) : NULL;
    if (!d || !ptr) return;
    int px = 120, py = 120;
    gdk_device_get_position(ptr, NULL, &px, &py);
    GdkMonitor *mon = gdk_display_get_monitor_at_point(d, px, py);
    GdkRectangle wa = { 0, 0, 1920, 1080 };
    if (mon) gdk_monitor_get_workarea(mon, &wa);
    int w = 240, h = 44 + n_items * 34;
    int x = (px + 10 + w > wa.x + wa.width) ? (wa.x + wa.width - w - 8) : (px + 10);
    int y = (py + 10 + h > wa.y + wa.height) ? (wa.y + wa.height - h - 8) : (py + 10);
    if (x < wa.x) x = wa.x + 8;
    if (y < wa.y) y = wa.y + 8;
    gtk_window_move(win, x, y);
}

void tomoyo_linux_super_panel_show(
    const char *header, const char *const *labels, size_t n,
    uint32_t bg_rgb, uint32_t fg_rgb, uint32_t muted_rgb,
    uint32_t border_rgb, uint32_t accent_rgb, const char *font_css,
    tomoyo_sp_pick_cb on_pick, void *ctx
) {
    tomoyo_linux_super_panel_close();
    if (!labels || n == 0) return;
    if (n > 6) n = 6;
    GtkWidget *win = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_widget_set_name(win, "tomoyo-sp");
    gtk_window_set_title(GTK_WINDOW(win), "Tomoyo Super Panel");
    gtk_window_set_decorated(GTK_WINDOW(win), FALSE);
    gtk_window_set_resizable(GTK_WINDOW(win), FALSE);
    gtk_window_set_skip_taskbar_hint(GTK_WINDOW(win), TRUE);
    gtk_window_set_skip_pager_hint(GTK_WINDOW(win), TRUE);
    gtk_window_set_keep_above(GTK_WINDOW(win), TRUE);
    gtk_widget_set_size_request(win, 240, -1);
    apply_sp_css(win, bg_rgb, fg_rgb, muted_rgb, border_rgb, accent_rgb, font_css);

    GtkWidget *vbox = gtk_box_new(GTK_ORIENTATION_VERTICAL, 2);
    gtk_container_set_border_width(GTK_CONTAINER(vbox), 6);
    GtkWidget *hdr = gtk_label_new(header ? header : "Tomoyo");
    gtk_widget_set_name(hdr, "tomoyo-sp-hdr");
    gtk_label_set_xalign(GTK_LABEL(hdr), 0.0f);
    gtk_label_set_ellipsize(GTK_LABEL(hdr), PANGO_ELLIPSIZE_END);
    gtk_label_set_max_width_chars(GTK_LABEL(hdr), 28);
    gtk_box_pack_start(GTK_BOX(vbox), hdr, FALSE, FALSE, 0);

    GtkWidget *first_btn = NULL;
    for (size_t i = 0; i < n; i++) {
        char buf[192];
        snprintf(buf, sizeof(buf), "%zu. %s", i + 1, labels[i] ? labels[i] : "");
        GtkWidget *btn = gtk_button_new_with_label(buf);
        gtk_style_context_add_class(gtk_widget_get_style_context(btn), "tomoyo-sp-btn");
        GtkWidget *child = gtk_bin_get_child(GTK_BIN(btn));
        if (GTK_IS_LABEL(child)) {
            gtk_label_set_xalign(GTK_LABEL(child), 0.0f);
            gtk_label_set_ellipsize(GTK_LABEL(child), PANGO_ELLIPSIZE_END);
        }
        g_signal_connect(btn, "clicked", G_CALLBACK(on_btn_clicked), GINT_TO_POINTER((int)i));
        gtk_box_pack_start(GTK_BOX(vbox), btn, FALSE, FALSE, 0);
        if (!first_btn) first_btn = btn;
    }
    gtk_container_add(GTK_CONTAINER(win), vbox);
    g_signal_connect(win, "key-press-event", G_CALLBACK(on_sp_key), NULL);
    g_signal_connect(win, "focus-out-event", G_CALLBACK(on_sp_focus_out), NULL);

    g_sp_win = win; g_sp_cb = on_pick; g_sp_ctx = ctx; g_sp_count = n;
    g_sp_open_us = g_get_monotonic_time();
    position_near_pointer(GTK_WINDOW(win), (int)n);
    gtk_widget_show_all(win);
    gtk_window_present(GTK_WINDOW(win));
    if (first_btn) gtk_widget_grab_focus(first_btn);
}
