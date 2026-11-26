#include "shell_internal.h"
#include <cairo.h>
#ifdef GDK_WINDOWING_WAYLAND
#include <gdk/gdkwayland.h>
#endif
#include <pango/pangocairo.h>
#include <stdlib.h>
#include <string.h>

static int is_wayland_display(void) {
#ifdef GDK_WINDOWING_WAYLAND
    GdkDisplay *gd = gdk_display_get_default();
    return (gd && GDK_IS_WAYLAND_DISPLAY(gd)) ? 1 : 0;
#else
    return 0;
#endif
}

static void apply_cursor(tomoyo_app_t *a, int kind) {
    if (!a || !a->window || a->cur_cursor == kind) return;
    a->cur_cursor = kind;
    GdkWindow *gw = gtk_widget_get_window(a->window);
    if (!gw) return;
    if (kind == 0) { gdk_window_set_cursor(gw, NULL); return; }
    static const char *names[] = {
        NULL, "nw-resize", "ne-resize", "sw-resize", "se-resize",
        "w-resize", "e-resize", "n-resize", "s-resize", "col-resize"
    };
    const char *nm = (kind >= 1 && kind <= 9) ? names[kind] : NULL;
    GdkDisplay *dpy = gdk_window_get_display(gw);
    GdkCursor *c = (nm && dpy) ? gdk_cursor_new_from_name(dpy, nm) : NULL;
    if (!c && kind == 9 && dpy) c = gdk_cursor_new_from_name(dpy, "ew-resize");
    gdk_window_set_cursor(gw, c);
    if (c) g_object_unref(c);
}

static int hit_resize_edge(tomoyo_app_t *a, double x, double y, GdkWindowEdge *edge_out, int *cur_out) {
    if (!a || !a->darea) return 0;
    int w = gtk_widget_get_allocated_width(a->darea), h = gtk_widget_get_allocated_height(a->darea);
    if (w <= 0 || h <= 0) return 0;
    const double grip = 6.0;
    int l = x < grip, r = x >= w - grip, t = y < grip, b = y >= h - grip;
    if (!l && !r && !t && !b) return 0;
    if (l && t) { *edge_out = GDK_WINDOW_EDGE_NORTH_WEST; *cur_out = 1; }
    else if (r && t) { *edge_out = GDK_WINDOW_EDGE_NORTH_EAST; *cur_out = 2; }
    else if (l && b) { *edge_out = GDK_WINDOW_EDGE_SOUTH_WEST; *cur_out = 3; }
    else if (r && b) { *edge_out = GDK_WINDOW_EDGE_SOUTH_EAST; *cur_out = 4; }
    else if (l) { *edge_out = GDK_WINDOW_EDGE_WEST; *cur_out = 5; }
    else if (r) { *edge_out = GDK_WINDOW_EDGE_EAST; *cur_out = 6; }
    else if (t) { *edge_out = GDK_WINDOW_EDGE_NORTH; *cur_out = 7; }
    else { *edge_out = GDK_WINDOW_EDGE_SOUTH; *cur_out = 8; }
    return 1;
}

static gboolean cb_entry_btn_press(GtkWidget *w, GdkEventButton *ev, gpointer ud) {
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (!a || !a->on_mouse || ev->button != 3) return FALSE;
    if (ev->type != GDK_BUTTON_PRESS) return TRUE;
    a->last_btn = (gint)ev->button;
    a->last_rx = (gint)ev->x_root; a->last_ry = (gint)ev->y_root; a->last_time = ev->time;
    if (!gtk_widget_has_focus(w)) {
        int s = 0, e = 0;
        gboolean sel = gtk_editable_get_selection_bounds(GTK_EDITABLE(w), &s, &e);
        gtk_widget_grab_focus(w);
        if (!sel) gtk_editable_set_position(GTK_EDITABLE(w), -1);
    }
    int ex = a->last_ex > 0 ? a->last_ex : 62, ey = a->last_ey > 0 ? a->last_ey : 14;
    a->popup_from_kb = 0;
    a->on_mouse(a->ctx, 7, ex + (int)ev->x, ey + (int)ev->y, 0);
    return TRUE;
}

static gboolean cb_entry_popup_menu(GtkWidget *w, gpointer ud) {
    (void)w;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (!a || !a->on_mouse) return FALSE;
    int ex = a->last_ex > 0 ? a->last_ex : 62, ey = a->last_ey > 0 ? a->last_ey : 14;
    a->last_time = gtk_get_current_event_time();
    a->popup_from_kb = 1;
    a->on_mouse(a->ctx, 7, ex + 24, ey + 28, 0);
    return TRUE;
}

static gboolean cb_draw(GtkWidget *w, cairo_t *cr, gpointer ud) {
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (a && a->on_draw) {
        a->on_draw(a->ctx, cr, gtk_widget_get_allocated_width(w), gtk_widget_get_allocated_height(w));
    }
    return FALSE;
}

static gboolean cb_key(GtkWidget *w, GdkEventKey *ev, gpointer ud) {
    (void)w;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (a && a->on_key && a->on_key(a->ctx, ev->keyval, ev->state)) return TRUE;
    return FALSE;
}

static gboolean cb_btn_press(GtkWidget *w, GdkEventButton *ev, gpointer ud) {
    (void)w;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (!a || !a->on_mouse) return FALSE;
    a->last_btn = (gint)ev->button;
    a->last_rx = (gint)ev->x_root;
    a->last_ry = (gint)ev->y_root;
    a->last_time = ev->time;
    if (ev->button == 1 && ev->type == GDK_BUTTON_PRESS) {
        GdkWindowEdge edge = GDK_WINDOW_EDGE_EAST;
        int cur = 0;
        if (hit_resize_edge(a, ev->x, ev->y, &edge, &cur)) {
            a->in_user_resize = 1;
            a->user_resize_until = g_get_monotonic_time() + 300000;
            gtk_window_begin_resize_drag(GTK_WINDOW(a->window), edge, (gint)ev->button, (gint)ev->x_root, (gint)ev->y_root, ev->time);
            return TRUE;
        }
    }
    int kind = (ev->type == GDK_2BUTTON_PRESS && ev->button == 1) ? 2 : (ev->button == 3 ? 3 : 1);
    a->on_mouse(a->ctx, kind, (int)ev->x, (int)ev->y, 0);
    return TRUE;
}

static gboolean cb_btn_release(GtkWidget *w, GdkEventButton *ev, gpointer ud) {
    (void)w;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    a->in_user_resize = 0;
    if (a && a->on_mouse && ev->button == 1) a->on_mouse(a->ctx, 4, (int)ev->x, (int)ev->y, 0);
    return FALSE;
}

static gboolean cb_motion(GtkWidget *w, GdkEventMotion *ev, gpointer ud) {
    (void)w;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (!a) return FALSE;
    a->in_user_resize = 0;
    GdkWindowEdge edge = GDK_WINDOW_EDGE_EAST;
    int cur = 0;
    if (hit_resize_edge(a, ev->x, ev->y, &edge, &cur)) {
        apply_cursor(a, cur);
        return FALSE;
    }
    if (a->on_mouse) a->on_mouse(a->ctx, 5, (int)ev->x, (int)ev->y, 0);
    return FALSE;
}

static gboolean cb_leave(GtkWidget *w, GdkEventCrossing *ev, gpointer ud) {
    (void)w; (void)ev;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (a) apply_cursor(a, 0);
    return FALSE;
}

static gboolean cb_configure(GtkWidget *w, GdkEventConfigure *ev, gpointer ud) {
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (!a || !gtk_widget_get_visible(w) || ev->width <= 0 || ev->height <= 0) return FALSE;
    gint64 now = g_get_monotonic_time();
    int size_changed = (ev->width != a->last_w || ev->height != a->last_h);
    int is_prog_size = (ev->width == a->prog_w && ev->height == a->prog_h);
    int user_sz = size_changed && (!is_prog_size || a->in_user_resize || now < a->user_resize_until);
    int wl = is_wayland_display();
    int px = ev->x, py = ev->y;
    if (!wl) gtk_window_get_position(GTK_WINDOW(w), &px, &py);
    int pos_changed = (!wl && a->ever_shown && now >= a->show_until && (px != a->last_x || py != a->last_y));
    a->last_w = ev->width; a->last_h = ev->height;
    a->last_x = px; a->last_y = py;
    if (user_sz) {
        a->prog_w = ev->width; a->prog_h = ev->height;
        a->user_resize_until = now + 250000;
    }
    if ((user_sz || pos_changed) && a->on_configure) {
        a->on_configure(a->ctx, px, py, ev->width, ev->height, (user_sz ? 1 : 0) | (pos_changed ? 2 : 0));
    }
    return FALSE;
}

static gboolean cb_scroll(GtkWidget *w, GdkEventScroll *ev, gpointer ud) {
    (void)w;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (!a || !a->on_mouse) return FALSE;
    int delta = (ev->direction == GDK_SCROLL_UP) ? 1 : (ev->direction == GDK_SCROLL_DOWN ? -1 : (ev->delta_y < 0 ? 1 : -1));
    a->on_mouse(a->ctx, 6, (int)ev->x, (int)ev->y, delta);
    return TRUE;
}

static void cb_changed(GtkEditable *ed, gpointer ud) {
    (void)ed;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (a && !a->suppress_change && a->on_text_changed) a->on_text_changed(a->ctx);
}

static gboolean cb_paste(GtkEntry *e, gpointer ud) {
    (void)e;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (a && a->on_paste) a->on_paste(a->ctx);
    return FALSE;
}

static gboolean cb_focus_out(GtkWidget *w, GdkEventFocus *ev, gpointer ud) {
    (void)w; (void)ev;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (a && a->on_focus_out) a->on_focus_out(a->ctx);
    return FALSE;
}

static void cb_clip_owner(GtkClipboard *cb, GdkEvent *ev, gpointer ud) {
    (void)cb; (void)ev;
    tomoyo_app_t *a = (tomoyo_app_t *)ud;
    if (a && a->on_clip_change) a->on_clip_change(a->ctx);
}

tomoyo_app_t *tomoyo_linux_ui_create(
    void *ctx, tomoyo_draw_cb on_draw, tomoyo_key_cb on_key, tomoyo_mouse_cb on_mouse,
    tomoyo_configure_cb on_configure, tomoyo_void_cb on_text_changed, tomoyo_void_cb on_paste,
    tomoyo_void_cb on_focus_out, tomoyo_void_cb on_clip_change, tomoyo_cmd_cb on_tray_cmd
) {
    g_set_prgname("tomoyo");
    g_set_application_name("Tomoyo");
    if (!gtk_init_check(NULL, NULL)) return NULL;
    gdk_set_program_class("tomoyo");
    gtk_window_set_default_icon_name("tomoyo");
    tomoyo_app_t *a = (tomoyo_app_t *)calloc(1, sizeof(tomoyo_app_t));
    if (!a) return NULL;
    a->ctx = ctx; a->on_draw = on_draw; a->on_key = on_key; a->on_mouse = on_mouse;
    a->on_configure = on_configure; a->on_text_changed = on_text_changed; a->on_paste = on_paste;
    a->on_focus_out = on_focus_out; a->on_clip_change = on_clip_change; a->on_tray_cmd = on_tray_cmd;

    a->window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_widget_set_name(a->window, "tomoyo-shell");
    gtk_window_set_title(GTK_WINDOW(a->window), "Tomoyo");
    gtk_window_set_icon_name(GTK_WINDOW(a->window), "tomoyo");
    GdkScreen *scr = gdk_screen_get_default();
    GdkVisual *vis = scr ? gdk_screen_get_rgba_visual(scr) : NULL;
    if (vis) gtk_widget_set_visual(a->window, vis);
    gtk_widget_set_app_paintable(a->window, TRUE);
    GtkWidget *zero_hb = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
    gtk_widget_set_size_request(zero_hb, 0, 0);
    gtk_widget_set_no_show_all(zero_hb, TRUE);
    gtk_window_set_titlebar(GTK_WINDOW(a->window), zero_hb);
    gtk_window_set_decorated(GTK_WINDOW(a->window), FALSE);
    gtk_window_set_resizable(GTK_WINDOW(a->window), TRUE);
    GdkGeometry geom = {0};
    geom.min_width = 520; geom.min_height = 92;
    geom.max_width = 1600; geom.max_height = 1080;
    gtk_window_set_geometry_hints(GTK_WINDOW(a->window), NULL, &geom, GDK_HINT_MIN_SIZE | GDK_HINT_MAX_SIZE);
    gtk_window_set_skip_taskbar_hint(GTK_WINDOW(a->window), TRUE);
    gtk_window_set_skip_pager_hint(GTK_WINDOW(a->window), TRUE);
    gtk_window_set_keep_above(GTK_WINDOW(a->window), TRUE);
    gtk_window_set_default_size(GTK_WINDOW(a->window), 840, 92);

    a->overlay = gtk_overlay_new();
    a->darea = gtk_drawing_area_new();
    gtk_widget_add_events(a->darea, GDK_BUTTON_PRESS_MASK | GDK_BUTTON_RELEASE_MASK | GDK_POINTER_MOTION_MASK | GDK_LEAVE_NOTIFY_MASK | GDK_SCROLL_MASK | GDK_SMOOTH_SCROLL_MASK);
    a->fixed = gtk_fixed_new();
    a->entry = gtk_entry_new();
    gtk_widget_set_name(a->entry, "tomoyo-input");
    gtk_entry_set_has_frame(GTK_ENTRY(a->entry), FALSE);
    gtk_fixed_put(GTK_FIXED(a->fixed), a->entry, 62, 14);
    gtk_widget_set_size_request(a->entry, 140, 34);

    gtk_container_add(GTK_CONTAINER(a->overlay), a->darea);
    gtk_overlay_add_overlay(GTK_OVERLAY(a->overlay), a->fixed);
    gtk_overlay_set_overlay_pass_through(GTK_OVERLAY(a->overlay), a->fixed, TRUE);
    gtk_container_add(GTK_CONTAINER(a->window), a->overlay);

    a->css = gtk_css_provider_new();
    gtk_style_context_add_provider(gtk_widget_get_style_context(a->window), GTK_STYLE_PROVIDER(a->css), GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
    gtk_style_context_add_provider(gtk_widget_get_style_context(a->entry), GTK_STYLE_PROVIDER(a->css), GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);

    g_signal_connect(a->darea, "draw", G_CALLBACK(cb_draw), a);
    g_signal_connect(a->darea, "button-press-event", G_CALLBACK(cb_btn_press), a);
    g_signal_connect(a->darea, "button-release-event", G_CALLBACK(cb_btn_release), a);
    g_signal_connect(a->darea, "motion-notify-event", G_CALLBACK(cb_motion), a);
    g_signal_connect(a->darea, "leave-notify-event", G_CALLBACK(cb_leave), a);
    g_signal_connect(a->darea, "scroll-event", G_CALLBACK(cb_scroll), a);
    g_signal_connect(a->window, "configure-event", G_CALLBACK(cb_configure), a);
    g_signal_connect(a->entry, "key-press-event", G_CALLBACK(cb_key), a);
    g_signal_connect(a->entry, "button-press-event", G_CALLBACK(cb_entry_btn_press), a);
    g_signal_connect(a->entry, "popup-menu", G_CALLBACK(cb_entry_popup_menu), a);
    g_signal_connect(a->window, "key-press-event", G_CALLBACK(cb_key), a);
    a->entry_changed_id = g_signal_connect(a->entry, "changed", G_CALLBACK(cb_changed), a);
    g_signal_connect(a->entry, "paste-clipboard", G_CALLBACK(cb_paste), a);
    g_signal_connect(a->window, "focus-out-event", G_CALLBACK(cb_focus_out), a);
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (cb) g_signal_connect(cb, "owner-change", G_CALLBACK(cb_clip_owner), a);
    return a;
}

void tomoyo_linux_ui_set_theme(tomoyo_app_t *a, uint32_t text_rgb, uint32_t bg_rgb, int font_px) {
    if (!a) return;
    char css[768];
    snprintf(css, sizeof(css),
        "window#tomoyo-shell { background-color: transparent; background-image: none; border: none; box-shadow: none; } "
        "entry#tomoyo-input { background: transparent; background-color: rgba(%u,%u,%u,0.01); "
        "color: #%06x; caret-color: #%06x; border: none; box-shadow: none; outline: none; "
        "font-family: \"阿里巴巴普惠体 3.0 55 Regular\", \"Noto Sans CJK SC\", sans-serif; font-size: %dpx; padding: 0; }",
        (bg_rgb >> 16) & 0xff, (bg_rgb >> 8) & 0xff, bg_rgb & 0xff, text_rgb & 0xffffff, text_rgb & 0xffffff, font_px);
    gtk_css_provider_load_from_data(a->css, css, -1, NULL);
}

void tomoyo_linux_ui_begin_drag(tomoyo_app_t *a) {
    if (!a || !a->window || !a->last_btn) return;
    gtk_window_begin_move_drag(GTK_WINDOW(a->window), a->last_btn, a->last_rx, a->last_ry, a->last_time);
}

void tomoyo_linux_ui_set_entry_bounds(tomoyo_app_t *a, int x, int y, int w, int h) {
    if (!a) return;
    int cw = w > 40 ? w : 40;
    int ch = h > 16 ? h : 16;
    if (x == a->last_ex && y == a->last_ey && cw == a->last_ew && ch == a->last_eh) return;
    if (x != a->last_ex || y != a->last_ey) gtk_fixed_move(GTK_FIXED(a->fixed), a->entry, x, y);
    if (cw != a->last_ew || ch != a->last_eh) gtk_widget_set_size_request(a->entry, cw, ch);
    a->last_ex = x; a->last_ey = y; a->last_ew = cw; a->last_eh = ch;
}

void tomoyo_linux_ui_set_entry_text(tomoyo_app_t *a, const char *txt) {
    if (!a) return;
    a->suppress_change = 1;
    gtk_entry_set_text(GTK_ENTRY(a->entry), txt ? txt : "");
    gtk_editable_set_position(GTK_EDITABLE(a->entry), -1);
    a->suppress_change = 0;
}

const char *tomoyo_linux_ui_get_entry_text(tomoyo_app_t *a) {
    return a ? gtk_entry_get_text(GTK_ENTRY(a->entry)) : "";
}

void tomoyo_linux_ui_set_placeholder(tomoyo_app_t *a, const char *txt) {
    if (a) gtk_entry_set_placeholder_text(GTK_ENTRY(a->entry), txt ? txt : "");
}

void tomoyo_linux_ui_set_cursor(tomoyo_app_t *a, int kind) { apply_cursor(a, kind); }

int tomoyo_linux_ui_is_wayland(void) { return is_wayland_display(); }

void tomoyo_linux_ui_get_size(tomoyo_app_t *a, int *ow, int *oh) {
    if (a && a->window) gtk_window_get_size(GTK_WINDOW(a->window), ow, oh);
    else { if (ow) *ow = 0; if (oh) *oh = 0; }
}

void tomoyo_linux_ui_show(tomoyo_app_t *a, int x, int y, int w, int h) {
    if (!a) return;
    a->prog_x = x; a->prog_y = y; a->prog_w = w; a->prog_h = h;
    a->last_w = w; a->last_h = h; a->last_x = x; a->last_y = y;
    a->ever_shown = 1;
    a->show_until = g_get_monotonic_time() + 180000;
    gtk_window_resize(GTK_WINDOW(a->window), w, h);
    if (!is_wayland_display()) gtk_window_move(GTK_WINDOW(a->window), x, y);
    gtk_widget_show_all(a->window);
    gtk_window_present(GTK_WINDOW(a->window));
    gtk_widget_grab_focus(a->entry);
}

void tomoyo_linux_ui_resize(tomoyo_app_t *a, int w, int h) {
    if (!a || a->in_user_resize || g_get_monotonic_time() < a->user_resize_until) return;
    if (w == a->last_w && h == a->last_h) return;
    a->prog_w = w; a->prog_h = h;
    a->last_w = w; a->last_h = h;
    gtk_window_resize(GTK_WINDOW(a->window), w, h);
}

void tomoyo_linux_ui_hide(tomoyo_app_t *a) {
    if (a) gtk_widget_hide(a->window);
}

int tomoyo_linux_ui_is_visible(tomoyo_app_t *a) {
    return (a && gtk_widget_get_visible(a->window)) ? 1 : 0;
}

void tomoyo_linux_ui_queue_draw(tomoyo_app_t *a) {
    if (a) gtk_widget_queue_draw(a->darea);
}

void tomoyo_linux_ui_screen_info(tomoyo_app_t *a, int *ox, int *oy, int *ow, int *oh, int *oscale) {
    GdkDisplay *d = gdk_display_get_default();
    GdkMonitor *m = d ? gdk_display_get_primary_monitor(d) : NULL;
    if (a && d && a->window && gtk_widget_get_window(a->window)) {
        GdkMonitor *cur = gdk_display_get_monitor_at_window(d, gtk_widget_get_window(a->window));
        if (cur) m = cur;
    }
    if (m) {
        GdkRectangle rc;
        gdk_monitor_get_workarea(m, &rc);
        *ox = rc.x; *oy = rc.y; *ow = rc.width; *oh = rc.height;
        int sf = gdk_monitor_get_scale_factor(m);
        *oscale = sf > 0 ? sf : 1;
    } else {
        *ox = 0; *oy = 0; *ow = 1920; *oh = 1080; *oscale = 1;
    }
}

void tomoyo_linux_ui_focus_entry(tomoyo_app_t *a) {
    if (!a || !a->window || !a->entry) return;
    int s = 0, e = 0;
    gboolean had_sel = gtk_editable_get_selection_bounds(GTK_EDITABLE(a->entry), &s, &e);
    int pos = gtk_editable_get_position(GTK_EDITABLE(a->entry));
    gtk_window_present(GTK_WINDOW(a->window));
    gtk_widget_grab_focus(a->entry);
    if (had_sel) gtk_editable_select_region(GTK_EDITABLE(a->entry), s, e);
    else gtk_editable_set_position(GTK_EDITABLE(a->entry), pos);
}
int tomoyo_linux_ui_is_active(tomoyo_app_t *a) { return (a && a->window && gtk_window_is_active(GTK_WINDOW(a->window))) ? 1 : 0; }
int tomoyo_linux_entry_has_selection(tomoyo_app_t *a) {
    int s = 0, e = 0;
    return (a && a->entry && gtk_editable_get_selection_bounds(GTK_EDITABLE(a->entry), &s, &e)) ? 1 : 0;
}
void tomoyo_linux_entry_edit_cmd(tomoyo_app_t *a, int op) {
    if (!a || !a->entry) return;
    if (op == 1) gtk_editable_cut_clipboard(GTK_EDITABLE(a->entry));
    else if (op == 2) gtk_editable_copy_clipboard(GTK_EDITABLE(a->entry));
    else if (op == 3) gtk_editable_paste_clipboard(GTK_EDITABLE(a->entry));
    else if (op == 4) gtk_editable_select_region(GTK_EDITABLE(a->entry), 0, -1);
}

typedef struct { void (*fn)(void *); void *data; } ui_idle_t;
static gboolean run_ui_idle(gpointer ud) {
    ui_idle_t *t = (ui_idle_t *)ud;
    if (t && t->fn) t->fn(t->data);
    free(t);
    return G_SOURCE_REMOVE;
}
void tomoyo_linux_schedule_idle(void (*fn)(void *), void *data) {
    ui_idle_t *t = (ui_idle_t *)malloc(sizeof(ui_idle_t));
    if (t) { t->fn = fn; t->data = data; g_idle_add(run_ui_idle, t); }
}

typedef struct { int (*fn)(void *); void *data; } ui_timer_t;
static gboolean run_timer_cb(gpointer ud) {
    ui_timer_t *t = (ui_timer_t *)ud;
    return (t && t->fn && t->fn(t->data)) ? G_SOURCE_CONTINUE : G_SOURCE_REMOVE;
}
uint32_t tomoyo_linux_add_timeout(uint32_t ms, int (*fn)(void *), void *data) {
    ui_timer_t *t = (ui_timer_t *)malloc(sizeof(ui_timer_t));
    if (!t) return 0;
    t->fn = fn; t->data = data;
    return g_timeout_add_full(G_PRIORITY_DEFAULT, ms, run_timer_cb, t, free);
}
void tomoyo_linux_remove_source(uint32_t id) { if (id) g_source_remove(id); }
void tomoyo_linux_ui_run(void) { gtk_main(); }
void tomoyo_linux_ui_quit(void) { gtk_main_quit(); }
void tomoyo_linux_ui_destroy(tomoyo_app_t *a) {
    if (!a) return;
    if (a->tray) g_object_unref(a->tray);
    if (a->css) g_object_unref(a->css);
    if (a->window) gtk_widget_destroy(a->window);
    free(a);
}
