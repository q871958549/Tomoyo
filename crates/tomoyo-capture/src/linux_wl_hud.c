#define _GNU_SOURCE
#include <gtk/gtk.h>
#include <gdk/gdkwayland.h>
#include <wayland-client.h>
#include <X11/Xlib.h>
#include <X11/extensions/shape.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef void (*hud_draw_cb)(void *ctx, void *cr, int w, int h);
typedef int (*hud_mouse_cb)(void *ctx, int kind, int x, int y, int btn);
typedef int (*hud_tick_cb)(void *ctx);

static const struct wl_interface zwlr_layer_surface_v1_iface;
static const struct wl_interface zwlr_layer_shell_v1_iface;

static const struct wl_interface *g_ls_types[] = {
    NULL, NULL, NULL, NULL, NULL,
    &zwlr_layer_surface_v1_iface, &wl_surface_interface, &wl_output_interface, NULL, NULL
};
static const struct wl_message g_ls_shell_reqs[] = {
    { "get_layer_surface", "no?ous", g_ls_types + 5 },
    { "destroy", "3", g_ls_types }
};
static const struct wl_interface zwlr_layer_shell_v1_iface = {
    "zwlr_layer_shell_v1", 4, 2, g_ls_shell_reqs, 0, NULL
};
static const struct wl_message g_ls_surf_reqs[] = {
    { "set_size", "uu", g_ls_types },
    { "set_anchor", "u", g_ls_types },
    { "set_exclusive_zone", "i", g_ls_types },
    { "set_margin", "iiii", g_ls_types },
    { "set_keyboard_interactivity", "u", g_ls_types },
    { "get_popup", "o", g_ls_types },
    { "ack_configure", "u", g_ls_types },
    { "destroy", "", g_ls_types },
    { "set_layer", "2u", g_ls_types }
};
static const struct wl_message g_ls_surf_evts[] = {
    { "configure", "uuu", g_ls_types },
    { "closed", "", g_ls_types }
};
static const struct wl_interface zwlr_layer_surface_v1_iface = {
    "zwlr_layer_surface_v1", 4, 9, g_ls_surf_reqs, 2, g_ls_surf_evts
};

static struct wl_proxy *g_wl_shell = NULL;
static struct wl_compositor *g_wl_comp = NULL;
static int g_wl_probed = 0;

static void wl_reg_global(void *d, struct wl_registry *reg, uint32_t id, const char *iface, uint32_t ver) {
    (void)d;
    if (strcmp(iface, "zwlr_layer_shell_v1") == 0 && !g_wl_shell) {
        g_wl_shell = (struct wl_proxy *)wl_registry_bind(reg, id, &zwlr_layer_shell_v1_iface, ver < 4 ? ver : 4);
    } else if (strcmp(iface, "wl_compositor") == 0 && !g_wl_comp) {
        g_wl_comp = (struct wl_compositor *)wl_registry_bind(reg, id, &wl_compositor_interface, ver < 4 ? ver : 4);
    }
}
static void wl_reg_rem(void *d, struct wl_registry *r, uint32_t id) { (void)d; (void)r; (void)id; }
static const struct wl_registry_listener g_reg_ls = { wl_reg_global, wl_reg_rem };

static int ensure_wl_shell(void) {
    if (!gtk_init_check(NULL, NULL)) return 0;
    GdkDisplay *gd = gdk_display_get_default();
    if (!gd || !GDK_IS_WAYLAND_DISPLAY(gd)) return 0;
    if (!g_wl_probed) {
        g_wl_probed = 1;
        struct wl_display *wd = gdk_wayland_display_get_wl_display(gd);
        if (wd) {
            struct wl_registry *reg = wl_display_get_registry(wd);
            wl_registry_add_listener(reg, &g_reg_ls, NULL);
            wl_display_roundtrip(wd);
            wl_registry_destroy(reg);
        }
    }
    return g_wl_shell != NULL;
}

typedef struct {
    struct wl_proxy *ls;
    GtkWidget *win;
    int configured;
    int allow_resize;
} wl_ls_t;

static void ls_on_conf(void *d, struct wl_proxy *ls, uint32_t serial, uint32_t w, uint32_t h) {
    wl_ls_t *s = (wl_ls_t *)d;
    wl_proxy_marshal_flags(ls, 6, NULL, wl_proxy_get_version(ls), 0, serial);
    if (s) {
        s->configured = 1;
        if (s->allow_resize && s->win && GTK_IS_WINDOW(s->win) && w > 0 && h > 0) {
            gtk_widget_set_size_request(s->win, (gint)w, (gint)h);
            gtk_window_resize(GTK_WINDOW(s->win), (gint)w, (gint)h);
        }
    }
}
static void ls_on_closed(void *d, struct wl_proxy *ls) { (void)d; (void)ls; }
static const void *g_ls_cbs[2] = { (void *)ls_on_conf, (void *)ls_on_closed };

static struct wl_proxy *attach_layer_surface(GtkWidget *win, int layer, int w, int h, int x, int y, int kb, const char *ns, wl_ls_t *state) {
    GdkScreen *scr = gdk_screen_get_default();
    GdkVisual *vis = scr ? gdk_screen_get_rgba_visual(scr) : NULL;
    if (vis) gtk_widget_set_visual(win, vis);
    gtk_widget_set_app_paintable(win, TRUE);
    gtk_window_set_decorated(GTK_WINDOW(win), FALSE);
    gtk_window_set_resizable(GTK_WINDOW(win), FALSE);
    gtk_widget_set_size_request(win, w, h);
    gtk_widget_realize(win);
    GdkWindow *gw = gtk_widget_get_window(win);
    if (!gw || !g_wl_shell) return NULL;
    gdk_wayland_window_set_use_custom_surface(gw);
    struct wl_surface *ws = gdk_wayland_window_get_wl_surface(gw);
    if (!ws) return NULL;
    struct wl_proxy *ls = wl_proxy_marshal_flags(
        g_wl_shell, 0, &zwlr_layer_surface_v1_iface, wl_proxy_get_version(g_wl_shell),
        0, NULL, ws, NULL, (uint32_t)layer, ns);
    if (!ls) return NULL;
    state->ls = ls;
    state->configured = 0;
    wl_proxy_add_listener(ls, (void (**)(void))g_ls_cbs, state);
    wl_proxy_marshal_flags(ls, 0, NULL, wl_proxy_get_version(ls), 0, (uint32_t)w, (uint32_t)h);
    wl_proxy_marshal_flags(ls, 1, NULL, wl_proxy_get_version(ls), 0, 1u | 4u); /* TOP | LEFT */
    wl_proxy_marshal_flags(ls, 2, NULL, wl_proxy_get_version(ls), 0, -1);
    wl_proxy_marshal_flags(ls, 3, NULL, wl_proxy_get_version(ls), 0, y, 0, 0, x);
    wl_proxy_marshal_flags(ls, 4, NULL, wl_proxy_get_version(ls), 0, (uint32_t)kb);
    wl_surface_commit(ws);
    struct wl_display *wd = gdk_wayland_display_get_wl_display(gdk_display_get_default());
    if (wd) wl_display_roundtrip(wd);
    return ls;
}

static void make_click_through(GtkWidget *win) {
    GdkWindow *gw = gtk_widget_get_window(win);
    if (!gw) return;
    cairo_region_t *empty = cairo_region_create();
    gdk_window_input_shape_combine_region(gw, empty, 0, 0);
    cairo_region_destroy(empty);
    if (GDK_IS_WAYLAND_DISPLAY(gdk_display_get_default()) && g_wl_comp) {
        struct wl_surface *ws = gdk_wayland_window_get_wl_surface(gw);
        if (ws) {
            struct wl_region *reg = wl_compositor_create_region(g_wl_comp);
            wl_surface_set_input_region(ws, reg);
            wl_region_destroy(reg);
            wl_surface_commit(ws);
        }
    }
}

static void update_layer_geom(GtkWidget *win, struct wl_proxy *ls, int w, int h, int x, int y) {
    if (!ls) return;
    wl_proxy_marshal_flags(ls, 0, NULL, wl_proxy_get_version(ls), 0, (uint32_t)w, (uint32_t)h);
    wl_proxy_marshal_flags(ls, 3, NULL, wl_proxy_get_version(ls), 0, y, 0, 0, x);
    GdkWindow *gw = gtk_widget_get_window(win);
    struct wl_surface *ws = gw ? gdk_wayland_window_get_wl_surface(gw) : NULL;
    if (ws) wl_surface_commit(ws);
    GdkDisplay *gd = gdk_display_get_default();
    if (gd) gdk_display_flush(gd);
}

static void destroy_layer_surface(struct wl_proxy **ls) {
    if (ls && *ls) {
        wl_proxy_marshal_flags(*ls, 7, NULL, wl_proxy_get_version(*ls), WL_MARSHAL_FLAG_DESTROY);
        *ls = NULL;
    }
}

void *tomoyo_cap_attach_fullscreen_overlay(GtkWidget *win) {
    if (!ensure_wl_shell()) return NULL;
    GdkDisplay *gd = gdk_display_get_default();
    if (!gd || gdk_display_get_n_monitors(gd) != 1) return NULL;
    GdkMonitor *mon = gdk_display_get_primary_monitor(gd);
    if (!mon) mon = gdk_display_get_monitor(gd, 0);
    GdkRectangle mg = {0, 0, 0, 0};
    if (mon) gdk_monitor_get_geometry(mon, &mg);
    gtk_window_set_decorated(GTK_WINDOW(win), FALSE);
    gtk_window_set_resizable(GTK_WINDOW(win), FALSE);
    if (mg.width > 0 && mg.height > 0) {
        gtk_window_set_default_size(GTK_WINDOW(win), mg.width, mg.height);
        gtk_widget_set_size_request(win, mg.width, mg.height);
        gtk_window_resize(GTK_WINDOW(win), mg.width, mg.height);
    }
    gtk_widget_realize(win);
    GdkWindow *gw = gtk_widget_get_window(win);
    struct wl_surface *ws = gw ? gdk_wayland_window_get_wl_surface(gw) : NULL;
    if (!gw || !ws) return NULL;
    gdk_wayland_window_set_use_custom_surface(gw);
    wl_ls_t *st = (wl_ls_t *)calloc(1, sizeof(wl_ls_t));
    if (!st) return NULL;
    st->win = win;
    st->allow_resize = 1;
    struct wl_proxy *ls = wl_proxy_marshal_flags(
        g_wl_shell, 0, &zwlr_layer_surface_v1_iface, wl_proxy_get_version(g_wl_shell),
        0, NULL, ws, NULL, 3u, "tomoyo-overlay");
    if (!ls) { free(st); return NULL; }
    st->ls = ls;
    wl_proxy_add_listener(ls, (void (**)(void))g_ls_cbs, st);
    wl_proxy_marshal_flags(ls, 0, NULL, wl_proxy_get_version(ls), 0, 0u, 0u);
    wl_proxy_marshal_flags(ls, 1, NULL, wl_proxy_get_version(ls), 0, 15u);
    wl_proxy_marshal_flags(ls, 2, NULL, wl_proxy_get_version(ls), 0, -1);
    wl_proxy_marshal_flags(ls, 4, NULL, wl_proxy_get_version(ls), 0, 1u);
    wl_surface_commit(ws);
    struct wl_display *wd = gdk_wayland_display_get_wl_display(gd);
    if (wd) wl_display_roundtrip(wd);
    return st;
}

void tomoyo_cap_detach_fullscreen_overlay(void **p) {
    if (!p || !*p) return;
    wl_ls_t *st = (wl_ls_t *)*p;
    st->win = NULL;
    st->allow_resize = 0;
    destroy_layer_surface(&st->ls);
    free(st);
    *p = NULL;
}

/* --- Recording Borders (Wayland Layer-Shell TOP or X11 fallback) --- */
static GtkWidget *g_wl_borders[4] = {0};
static wl_ls_t g_wl_border_ls[4];
static Display *g_x_dpy = NULL;
static Window g_x_strips[4] = {0};

static gboolean draw_border_strip(GtkWidget *w, cairo_t *cr, gpointer ud) {
    (void)w; (void)ud;
    cairo_set_operator(cr, CAIRO_OPERATOR_SOURCE);
    cairo_set_source_rgba(cr, 0.937, 0.267, 0.267, 0.96);
    cairo_paint(cr);
    return TRUE;
}

void tomoyo_cap_rec_borders_hide(void) {
    for (int i = 0; i < 4; i++) {
        destroy_layer_surface(&g_wl_border_ls[i].ls);
        if (g_wl_borders[i]) { gtk_widget_destroy(g_wl_borders[i]); g_wl_borders[i] = NULL; }
    }
    if (g_x_dpy) {
        for (int i = 0; i < 4; i++) if (g_x_strips[i]) { XDestroyWindow(g_x_dpy, g_x_strips[i]); g_x_strips[i] = 0; }
        XFlush(g_x_dpy);
        XCloseDisplay(g_x_dpy);
        g_x_dpy = NULL;
    }
    if (gtk_init_check(NULL, NULL)) {
        for (int i = 0; i < 12 && gtk_events_pending(); i++) gtk_main_iteration_do(FALSE);
        GdkDisplay *gd = gdk_display_get_default();
        if (gd) gdk_display_flush(gd);
    }
}

void tomoyo_cap_rec_borders_show(int x, int y, int w, int h, int snap_w, int snap_h) {
    tomoyo_cap_rec_borders_hide();
    if (w < 8 || h < 8) return;
    int sw = snap_w > 0 ? snap_w : w;
    int sh = snap_h > 0 ? snap_h : h;
    if (ensure_wl_shell()) {
        GdkDisplay *gd = gdk_display_get_default();
        GdkRectangle mg = {0, 0, sw, sh};
        if (gd && gdk_display_get_n_monitors(gd) > 0) gdk_monitor_get_geometry(gdk_display_get_primary_monitor(gd) ? gdk_display_get_primary_monitor(gd) : gdk_display_get_monitor(gd, 0), &mg);
        double sx = mg.width > 0 ? (double)mg.width / (double)sw : 1.0;
        double sy = mg.height > 0 ? (double)mg.height / (double)sh : 1.0;
        int lx = (int)(x * sx + 0.5), ly = (int)(y * sy + 0.5), lw = (int)(w * sx + 0.5), lh = (int)(h * sy + 0.5);
        int mw = mg.width > 0 ? mg.width : sw, mh = mg.height > 0 ? mg.height : sh;
        int b = 2, g = 2;
        int rx0 = lx - g - b; if (rx0 < 0) rx0 = 0;
        int ry0 = ly - g - b; if (ry0 < 0) ry0 = 0;
        int rx1 = lx + lw + g; if (rx1 > mw - b) rx1 = mw - b;
        int ry1 = ly + lh + g; if (ry1 > mh - b) ry1 = mh - b;
        int bw = (rx1 + b - rx0); if (bw < 8) bw = 8;
        int bh = (ry1 + b - ry0); if (bh < 8) bh = 8;
        int rects[4][4] = {
            { rx0, ry0, bw, b },
            { rx0, ry1, bw, b },
            { rx0, ry0 + b, b, bh - b * 2 > 2 ? bh - b * 2 : 2 },
            { rx1, ry0 + b, b, bh - b * 2 > 2 ? bh - b * 2 : 2 }
        };
        for (int i = 0; i < 4; i++) {
            g_wl_borders[i] = gtk_window_new(GTK_WINDOW_TOPLEVEL);
            g_signal_connect(g_wl_borders[i], "draw", G_CALLBACK(draw_border_strip), NULL);
            attach_layer_surface(g_wl_borders[i], 2, rects[i][2], rects[i][3], rects[i][0], rects[i][1], 0, "tomoyo-border", &g_wl_border_ls[i]);
            gtk_widget_show_all(g_wl_borders[i]);
            make_click_through(g_wl_borders[i]);
        }
        return;
    }
    g_x_dpy = XOpenDisplay(NULL);
    if (!g_x_dpy) return;
    Window root = DefaultRootWindow(g_x_dpy);
    int b = 2, rects[4][4] = { { x - b, y - b, w + b * 2, b }, { x - b, y + h, w + b * 2, b }, { x - b, y, b, h }, { x + w, y, b, h } };
    XSetWindowAttributes attr; memset(&attr, 0, sizeof(attr));
    attr.override_redirect = True; attr.background_pixel = 0xFFEF4444UL;
    for (int i = 0; i < 4; i++) {
        Window win = XCreateWindow(g_x_dpy, root, rects[i][0], rects[i][1], (unsigned)rects[i][2], (unsigned)rects[i][3],
                                   0, CopyFromParent, InputOutput, CopyFromParent, CWOverrideRedirect | CWBackPixel, &attr);
        XShapeCombineRectangles(g_x_dpy, win, ShapeInput, 0, 0, NULL, 0, ShapeSet, Unsorted);
        XMapRaised(g_x_dpy, win);
        g_x_strips[i] = win;
    }
    XFlush(g_x_dpy);
}

/* --- Transparent Always-On-Top Draggable Recording HUD --- */
static volatile sig_atomic_t g_sig_act = 0;
static void on_sig(int s) { g_sig_act = (s == SIGUSR1) ? 1 : 2; }
int tomoyo_cap_signal_action(void) { int v = (int)g_sig_act; g_sig_act = 0; return v; }
void tomoyo_cap_install_rec_signals(void) {
    struct sigaction sa; memset(&sa, 0, sizeof(sa)); sa.sa_handler = on_sig; sigemptyset(&sa.sa_mask);
    sigaction(SIGUSR1, &sa, NULL); sigaction(SIGTERM, &sa, NULL); sigaction(SIGINT, &sa, NULL);
}
int tomoyo_cap_pid_alive(int pid) { return pid > 1 && kill((pid_t)pid, 0) == 0; }
void tomoyo_cap_send_usr1(int pid) { if (pid > 1) kill((pid_t)pid, SIGUSR1); }

typedef struct {
    GtkWidget *win, *da;
    void *ctx;
    hud_draw_cb on_draw;
    hud_mouse_cb on_mouse;
    hud_tick_cb on_tick;
    wl_ls_t wl;
    int is_wl, lx, ly, lw, lh, max_w, max_h;
    int dragging;
    double drag_sx, drag_sy;
    guint tid;
} hud_win_t;

static hud_win_t *g_hud = NULL;

static gboolean hud_draw(GtkWidget *w, cairo_t *cr, gpointer ud) {
    hud_win_t *hw = (hud_win_t *)ud;
    cairo_save(cr); cairo_set_operator(cr, CAIRO_OPERATOR_CLEAR); cairo_paint(cr); cairo_restore(cr);
    if (hw && hw->on_draw) hw->on_draw(hw->ctx, cr, gtk_widget_get_allocated_width(w), gtk_widget_get_allocated_height(w));
    return TRUE;
}
static gboolean hud_btn(GtkWidget *w, GdkEventButton *ev, gpointer ud) {
    (void)w; hud_win_t *hw = (hud_win_t *)ud;
    if (!hw || !hw->on_mouse) return FALSE;
    int k = (ev->type == GDK_BUTTON_RELEASE) ? 4 : 1;
    if (k == 4) { hw->dragging = 0; }
    int rc = hw->on_mouse(hw->ctx, k, (int)ev->x, (int)ev->y, (int)ev->button);
    if (rc == 2 && k == 1 && ev->button == 1) {
        if (hw->is_wl && hw->wl.ls) { hw->dragging = 1; hw->drag_sx = ev->x; hw->drag_sy = ev->y; }
        else gtk_window_begin_move_drag(GTK_WINDOW(hw->win), 1, (gint)ev->x_root, (gint)ev->y_root, ev->time);
    } else if (rc == 1) { gtk_main_quit(); }
    return TRUE;
}
static gboolean hud_mot(GtkWidget *w, GdkEventMotion *ev, gpointer ud) {
    (void)w; hud_win_t *hw = (hud_win_t *)ud;
    if (!hw) return FALSE;
    if (hw->dragging && hw->is_wl && hw->wl.ls) {
        int nx = hw->lx + (int)(ev->x - hw->drag_sx), ny = hw->ly + (int)(ev->y - hw->drag_sy);
        if (hw->max_w > hw->lw + 8) { if (nx < 4) nx = 4; else if (nx > hw->max_w - hw->lw - 4) nx = hw->max_w - hw->lw - 4; }
        if (hw->max_h > hw->lh + 8) { if (ny < 4) ny = 4; else if (ny > hw->max_h - hw->lh - 4) ny = hw->max_h - hw->lh - 4; }
        if (nx != hw->lx || ny != hw->ly) { hw->lx = nx; hw->ly = ny; update_layer_geom(hw->win, hw->wl.ls, hw->lw, hw->lh, nx, ny); }
        return TRUE;
    }
    if (hw->on_mouse) hw->on_mouse(hw->ctx, 5, (int)ev->x, (int)ev->y, 0);
    return FALSE;
}
static gboolean hud_tick(gpointer ud) {
    hud_win_t *hw = (hud_win_t *)ud;
    if (hw && hw->on_tick && hw->on_tick(hw->ctx)) {
        if (hw->da) gtk_widget_queue_draw(hw->da);
        return G_SOURCE_CONTINUE;
    }
    if (hw) hw->tid = 0;
    return G_SOURCE_REMOVE;
}
static gboolean hud_del(GtkWidget *w, GdkEvent *e, gpointer ud) { (void)w; (void)e; (void)ud; gtk_main_quit(); return TRUE; }
static gboolean hud_key(GtkWidget *w, GdkEventKey *ev, gpointer ud) {
    (void)w; (void)ud; if (ev->keyval == 0xFF1B) { gtk_main_quit(); return TRUE; } return FALSE;
}

void tomoyo_cap_rec_hud_redraw(void) { if (g_hud && g_hud->da) gtk_widget_queue_draw(g_hud->da); }
void tomoyo_cap_rec_hud_resize(int w, int h) {
    if (!g_hud || !g_hud->win) return;
    g_hud->lw = w; g_hud->lh = h;
    if (g_hud->max_w > w + 8 && g_hud->lx > g_hud->max_w - w - 6) g_hud->lx = g_hud->max_w - w - 6;
    if (g_hud->max_h > h + 8 && g_hud->ly > g_hud->max_h - h - 6) g_hud->ly = g_hud->max_h - h - 6;
    gtk_widget_set_size_request(g_hud->win, w, h);
    gtk_window_resize(GTK_WINDOW(g_hud->win), w, h);
    if (g_hud->is_wl && g_hud->wl.ls) update_layer_geom(g_hud->win, g_hud->wl.ls, w, h, g_hud->lx, g_hud->ly);
}
void tomoyo_cap_rec_hud_quit(void) { gtk_main_quit(); }

void tomoyo_cap_rec_hud_run(
    const char *title, int w, int h, int x, int y, int snap_w, int snap_h,
    void *ctx, hud_draw_cb on_draw, hud_mouse_cb on_mouse, hud_tick_cb on_tick
) {
    if (!gtk_init_check(NULL, NULL)) return;
    int is_wl = ensure_wl_shell();
    GdkRectangle mg = {0, 0, snap_w > 0 ? snap_w : 1920, snap_h > 0 ? snap_h : 1080};
    GdkDisplay *gd = gdk_display_get_default();
    if (gd && gdk_display_get_n_monitors(gd) > 0) gdk_monitor_get_geometry(gdk_display_get_primary_monitor(gd) ? gdk_display_get_primary_monitor(gd) : gdk_display_get_monitor(gd, 0), &mg);
    double sx = (is_wl && snap_w > 0 && mg.width > 0) ? (double)mg.width / (double)snap_w : 1.0;
    double sy = (is_wl && snap_h > 0 && mg.height > 0) ? (double)mg.height / (double)snap_h : 1.0;
    hud_win_t hw; memset(&hw, 0, sizeof(hw));
    hw.ctx = ctx; hw.on_draw = on_draw; hw.on_mouse = on_mouse; hw.on_tick = on_tick;
    hw.is_wl = is_wl; hw.lw = w; hw.lh = h;
    hw.max_w = mg.width > 0 ? mg.width : 1920; hw.max_h = mg.height > 0 ? mg.height : 1080;
    hw.lx = (int)(x * sx + 0.5); hw.ly = (int)(y * sy + 0.5);
    if (hw.lx < 4) hw.lx = 4;
    if (hw.lx > hw.max_w - w - 4) hw.lx = hw.max_w - w - 4;
    if (hw.ly < 4) hw.ly = 4;
    if (hw.ly > hw.max_h - h - 4) hw.ly = hw.max_h - h - 4;
    g_hud = &hw;
    hw.win = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(hw.win), title ? title : "Tomoyo 录屏控制");
    GdkScreen *scr = gdk_screen_get_default();
    GdkVisual *vis = scr ? gdk_screen_get_rgba_visual(scr) : NULL;
    if (vis) gtk_widget_set_visual(hw.win, vis);
    gtk_widget_set_app_paintable(hw.win, TRUE);
    hw.da = gtk_drawing_area_new();
    gtk_widget_add_events(hw.da, GDK_BUTTON_PRESS_MASK | GDK_BUTTON_RELEASE_MASK | GDK_POINTER_MOTION_MASK);
    gtk_container_add(GTK_CONTAINER(hw.win), hw.da);
    g_signal_connect(hw.da, "draw", G_CALLBACK(hud_draw), &hw);
    g_signal_connect(hw.da, "button-press-event", G_CALLBACK(hud_btn), &hw);
    g_signal_connect(hw.da, "button-release-event", G_CALLBACK(hud_btn), &hw);
    g_signal_connect(hw.da, "motion-notify-event", G_CALLBACK(hud_mot), &hw);
    g_signal_connect(hw.win, "key-press-event", G_CALLBACK(hud_key), NULL);
    g_signal_connect(hw.win, "delete-event", G_CALLBACK(hud_del), NULL);
    if (is_wl) {
        attach_layer_surface(hw.win, 3, w, h, hw.lx, hw.ly, 0, "tomoyo-rec-hud", &hw.wl);
    } else {
        gtk_window_set_decorated(GTK_WINDOW(hw.win), FALSE);
        gtk_window_set_resizable(GTK_WINDOW(hw.win), FALSE);
        gtk_window_set_keep_above(GTK_WINDOW(hw.win), TRUE);
        gtk_window_set_skip_taskbar_hint(GTK_WINDOW(hw.win), TRUE);
        gtk_window_set_default_size(GTK_WINDOW(hw.win), w, h);
        gtk_widget_set_size_request(hw.win, w, h);
        gtk_window_move(GTK_WINDOW(hw.win), hw.lx, hw.ly);
    }
    gtk_widget_show_all(hw.win);
    hw.tid = g_timeout_add(150, hud_tick, &hw);
    gtk_main();
    if (hw.tid) { g_source_remove(hw.tid); hw.tid = 0; }
    destroy_layer_surface(&hw.wl.ls);
    gtk_widget_destroy(hw.win);
    tomoyo_cap_rec_borders_hide();
    g_hud = NULL;
}
