#include <gtk/gtk.h>
#include <cairo.h>
#include <pango/pangocairo.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef void (*cap_draw_cb)(void *ctx, void *cr, int w, int h);
typedef int (*cap_key_cb)(void *ctx, uint32_t keyval, uint32_t state);
typedef int (*cap_mouse_cb)(void *ctx, int kind, int x, int y, int delta, int btn);
typedef void (*cap_cmd_cb)(void *ctx, uint32_t cmd);
void *tomoyo_cap_attach_fullscreen_overlay(GtkWidget *win);
void tomoyo_cap_detach_fullscreen_overlay(void **p);

typedef struct {
    GtkWidget *win;
    GtkWidget *darea;
    int fullscreen;
    void *ctx;
    cap_draw_cb on_draw;
    cap_key_cb on_key;
    cap_mouse_cb on_mouse;
    cap_cmd_cb on_cmd;
} cap_win_t;

void tomoyo_cap_free(void *p) { if (p) free(p); }

static int ensure_gtk(void) { return gtk_init_check(NULL, NULL) ? 1 : 0; }

static void free_pixbuf_data(guchar *pixels, gpointer ud) { (void)ud; free(pixels); }

int tomoyo_cap_clip_set_bgra(const uint8_t *bgra, int w, int h) {
    if (!ensure_gtk() || !bgra || w <= 0 || h <= 0) return 0;
    size_t sz = (size_t)w * (size_t)h * 4;
    uint8_t *rgba = (uint8_t *)malloc(sz);
    if (!rgba) return 0;
    for (int y = 0; y < h; y++) {
        for (int x = 0; x < w; x++) {
            size_t i = ((size_t)y * w + x) * 4;
            rgba[i] = bgra[i + 2]; rgba[i + 1] = bgra[i + 1]; rgba[i + 2] = bgra[i]; rgba[i + 3] = bgra[i + 3];
        }
    }
    GdkPixbuf *pb = gdk_pixbuf_new_from_data(rgba, GDK_COLORSPACE_RGB, TRUE, 8, w, h, w * 4, free_pixbuf_data, NULL);
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (cb && pb) {
        gtk_clipboard_set_image(cb, pb);
        gtk_clipboard_set_can_store(cb, NULL, 0);
        gtk_clipboard_store(cb);
        GdkDisplay *d = gdk_display_get_default();
        if (d) gdk_display_flush(d);
        for (int i = 0; i < 20 && gtk_events_pending(); i++) gtk_main_iteration_do(FALSE);
    }
    if (pb) g_object_unref(pb);
    return (cb && pb) ? 1 : 0;
}

void tomoyo_cap_clip_set_text(const char *txt) {
    if (!ensure_gtk() || !txt) return;
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (cb) {
        gtk_clipboard_set_text(cb, txt, -1);
        gtk_clipboard_set_can_store(cb, NULL, 0);
        gtk_clipboard_store(cb);
        GdkDisplay *d = gdk_display_get_default();
        if (d) gdk_display_flush(d);
        for (int i = 0; i < 15 && gtk_events_pending(); i++) gtk_main_iteration_do(FALSE);
    }
}

char *tomoyo_cap_clip_get_text(void) {
    if (!ensure_gtk()) return NULL;
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    gchar *s = cb ? gtk_clipboard_wait_for_text(cb) : NULL;
    if (!s) return NULL;
    char *out = strdup(s); g_free(s);
    return out;
}

char *tomoyo_cap_clip_get_uris(void) {
    if (!ensure_gtk()) return NULL;
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (!cb || !gtk_clipboard_wait_is_uris_available(cb)) return NULL;
    gchar **u = gtk_clipboard_wait_for_uris(cb);
    if (!u) return NULL;
    GString *gs = g_string_new("");
    for (int i = 0; u[i]; i++) { g_string_append(gs, u[i]); g_string_append_c(gs, '\n'); }
    g_strfreev(u);
    char *out = strdup(gs->str); g_string_free(gs, TRUE);
    return out;
}

uint8_t *tomoyo_cap_clip_get_image_bgra(uint32_t *ow, uint32_t *oh, size_t *olen) {
    if (!ensure_gtk()) return NULL;
    GtkClipboard *cb = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (!cb || !gtk_clipboard_wait_is_image_available(cb)) return NULL;
    GdkPixbuf *pb = gtk_clipboard_wait_for_image(cb);
    if (!pb) return NULL;
    int w = gdk_pixbuf_get_width(pb), h = gdk_pixbuf_get_height(pb);
    int nc = gdk_pixbuf_get_n_channels(pb), rs = gdk_pixbuf_get_rowstride(pb);
    if (w <= 0 || h <= 0 || w > 4096 || h > 4096) { g_object_unref(pb); return NULL; }
    size_t sz = (size_t)w * (size_t)h * 4;
    uint8_t *bgra = (uint8_t *)malloc(sz);
    if (!bgra) { g_object_unref(pb); return NULL; }
    const guchar *px = gdk_pixbuf_get_pixels(pb);
    for (int y = 0; y < h; y++) {
        for (int x = 0; x < w; x++) {
            const guchar *s = px + y * rs + x * nc;
            size_t i = ((size_t)y * w + x) * 4;
            bgra[i] = s[2]; bgra[i + 1] = s[1]; bgra[i + 2] = s[0]; bgra[i + 3] = nc >= 4 ? s[3] : 255;
        }
    }
    g_object_unref(pb);
    *ow = (uint32_t)w; *oh = (uint32_t)h; *olen = sz;
    return bgra;
}

uint8_t *tomoyo_cap_x11_grab(int *ow, int *oh, size_t *olen) {
    Display *dpy = XOpenDisplay(NULL);
    if (!dpy) return NULL;
    int scr = DefaultScreen(dpy);
    int w = DisplayWidth(dpy, scr), h = DisplayHeight(dpy, scr);
    XImage *img = XGetImage(dpy, RootWindow(dpy, scr), 0, 0, (unsigned)w, (unsigned)h, AllPlanes, ZPixmap);
    if (!img) { XCloseDisplay(dpy); return NULL; }
    size_t sz = (size_t)w * (size_t)h * 4;
    uint8_t *bgra = (uint8_t *)malloc(sz);
    if (bgra) {
        for (int y = 0; y < h; y++) {
            for (int x = 0; x < w; x++) {
                unsigned long px = XGetPixel(img, x, y);
                size_t i = ((size_t)y * w + x) * 4;
                bgra[i] = (uint8_t)(px & 0xff);
                bgra[i + 1] = (uint8_t)((px >> 8) & 0xff);
                bgra[i + 2] = (uint8_t)((px >> 16) & 0xff);
                bgra[i + 3] = 255;
            }
        }
        *ow = w; *oh = h; *olen = sz;
    }
    XDestroyImage(img);
    XCloseDisplay(dpy);
    return bgra;
}

void tomoyo_cap_cairo_blit_bgra(void *cr_v, const uint8_t *bgra, int w, int h, int dw, int dh) {
    cairo_t *cr = (cairo_t *)cr_v;
    if (!cr || !bgra || w <= 0 || h <= 0) return;
    cairo_surface_t *s = cairo_image_surface_create_for_data((unsigned char *)bgra, CAIRO_FORMAT_ARGB32, w, h, w * 4);
    cairo_save(cr);
    cairo_set_operator(cr, CAIRO_OPERATOR_SOURCE);
    if (dw != w || dh != h) cairo_scale(cr, (double)dw / (double)w, (double)dh / (double)h);
    cairo_set_source_surface(cr, s, 0, 0);
    cairo_paint(cr);
    cairo_restore(cr);
    cairo_surface_destroy(s);
}

void tomoyo_cap_pango_draw(
    void *cr_v, const char *font_desc, uint32_t rgb, int align, int wrap,
    const char *text, double x, double y, double w, double h
) {
    cairo_t *cr = (cairo_t *)cr_v;
    if (!cr || !text || !text[0] || w <= 2.0 || h <= 2.0) return;
    PangoLayout *ly = pango_cairo_create_layout(cr);
    PangoFontDescription *desc = pango_font_description_from_string(font_desc);
    pango_layout_set_font_description(ly, desc);
    pango_layout_set_text(ly, text, -1);
    pango_layout_set_width(ly, (int)(w * PANGO_SCALE));
    pango_layout_set_alignment(ly, align == 1 ? PANGO_ALIGN_CENTER : PANGO_ALIGN_LEFT);
    if (!wrap) pango_layout_set_ellipsize(ly, PANGO_ELLIPSIZE_END);
    else { pango_layout_set_wrap(ly, PANGO_WRAP_WORD_CHAR); pango_layout_set_height(ly, (int)(h * PANGO_SCALE)); }
    int pw = 0, ph = 0; pango_layout_get_pixel_size(ly, &pw, &ph);
    double dy = wrap ? y : (y + (h - (double)ph) * 0.5);
    cairo_save(cr);
    cairo_set_source_rgb(cr, ((rgb >> 16) & 0xff) / 255.0, ((rgb >> 8) & 0xff) / 255.0, (rgb & 0xff) / 255.0);
    cairo_move_to(cr, x, dy);
    pango_cairo_show_layout(cr, ly);
    cairo_restore(cr);
    pango_font_description_free(desc);
    g_object_unref(ly);
}

uint8_t *tomoyo_cap_render_text_card(
    const char *text, const char *meta, int swatch_rgb, uint32_t bg, uint32_t fg, uint32_t muted,
    uint32_t border, double scale, int *ow, int *oh, size_t *olen
) {
    char fstr[96], mstr[96];
    snprintf(fstr, sizeof(fstr), "阿里巴巴普惠体 3.0 55 Regular %dpx", (int)(13.5 * scale));
    snprintf(mstr, sizeof(mstr), "阿里巴巴普惠体 3.0 55 Regular %dpx", (int)(11.0 * scale));
    if (swatch_rgb >= 0) {
        int cw = (int)(290.0 * scale + 0.5), ch = (int)(124.0 * scale + 0.5);
        cairo_surface_t *surf = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, cw, ch);
        cairo_t *cr = cairo_create(surf);
        cairo_set_source_rgb(cr, ((bg >> 16) & 0xff) / 255.0, ((bg >> 8) & 0xff) / 255.0, (bg & 0xff) / 255.0);
        cairo_paint(cr);
        cairo_set_source_rgb(cr, ((border >> 16) & 0xff) / 255.0, ((border >> 8) & 0xff) / 255.0, (border & 0xff) / 255.0);
        cairo_rectangle(cr, 0.5, 0.5, cw - 1.0, ch - 1.0); cairo_stroke(cr);
        cairo_set_source_rgb(cr, ((swatch_rgb >> 16) & 0xff) / 255.0, ((swatch_rgb >> 8) & 0xff) / 255.0, (swatch_rgb & 0xff) / 255.0);
        cairo_rectangle(cr, 16.0 * scale, 16.0 * scale, 72.0 * scale, 72.0 * scale); cairo_fill(cr);
        tomoyo_cap_pango_draw(cr, fstr, fg, 0, 0, text, 102.0 * scale, 24.0 * scale, cw - 116.0 * scale, 28.0 * scale);
        tomoyo_cap_pango_draw(cr, mstr, muted, 0, 0, meta, 102.0 * scale, 56.0 * scale, cw - 116.0 * scale, 24.0 * scale);
        cairo_surface_flush(surf);
        size_t sz = (size_t)cw * (size_t)ch * 4;
        uint8_t *out = (uint8_t *)malloc(sz);
        if (out) { memcpy(out, cairo_image_surface_get_data(surf), sz); *ow = cw; *oh = ch; *olen = sz; }
        cairo_destroy(cr); cairo_surface_destroy(surf);
        return out;
    }
    int max_tw = (int)(440.0 * scale), pad_x = (int)(20.0 * scale), pad_y = (int)(16.0 * scale);
    cairo_surface_t *tmp = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 16, 16);
    cairo_t *tcr = cairo_create(tmp);
    PangoLayout *ly = pango_cairo_create_layout(tcr);
    PangoFontDescription *desc = pango_font_description_from_string(fstr);
    pango_layout_set_font_description(ly, desc);
    pango_layout_set_text(ly, text, -1);
    pango_layout_set_width(ly, max_tw * PANGO_SCALE);
    pango_layout_set_wrap(ly, PANGO_WRAP_WORD_CHAR);
    int tw = 0, th = 0; pango_layout_get_pixel_size(ly, &tw, &th);
    pango_font_description_free(desc); g_object_unref(ly); cairo_destroy(tcr); cairo_surface_destroy(tmp);

    int cw = (tw + pad_x * 2); if (cw < (int)(240 * scale)) cw = (int)(240 * scale);
    int ch = th + pad_y * 2 + (int)(26.0 * scale); if (ch < (int)(64 * scale)) ch = (int)(64 * scale);
    if (ch > (int)(720 * scale)) ch = (int)(720 * scale);
    cairo_surface_t *surf = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, cw, ch);
    cairo_t *cr = cairo_create(surf);
    cairo_set_source_rgb(cr, ((bg >> 16) & 0xff) / 255.0, ((bg >> 8) & 0xff) / 255.0, (bg & 0xff) / 255.0);
    cairo_paint(cr);
    cairo_set_source_rgb(cr, ((border >> 16) & 0xff) / 255.0, ((border >> 8) & 0xff) / 255.0, (border & 0xff) / 255.0);
    cairo_rectangle(cr, 0.5, 0.5, cw - 1.0, ch - 1.0); cairo_stroke(cr);
    tomoyo_cap_pango_draw(cr, fstr, fg, 0, 1, text, pad_x, pad_y, cw - pad_x * 2.0, th + 6.0);
    tomoyo_cap_pango_draw(cr, mstr, muted, 0, 0, meta, pad_x, ch - (int)(24.0 * scale), cw - pad_x * 2.0, 20.0 * scale);
    cairo_surface_flush(surf);
    size_t sz = (size_t)cw * (size_t)ch * 4;
    uint8_t *out = (uint8_t *)malloc(sz);
    if (out) { memcpy(out, cairo_image_surface_get_data(surf), sz); *ow = cw; *oh = ch; *olen = sz; }
    cairo_destroy(cr); cairo_surface_destroy(surf);
    return out;
}

static gboolean on_cap_draw(GtkWidget *w, cairo_t *cr, gpointer ud) {
    cap_win_t *c = (cap_win_t *)ud;
    if (c && c->on_draw) c->on_draw(c->ctx, cr, gtk_widget_get_allocated_width(w), gtk_widget_get_allocated_height(w));
    return FALSE;
}
static gboolean on_cap_key(GtkWidget *w, GdkEventKey *ev, gpointer ud) {
    cap_win_t *c = (cap_win_t *)ud;
    guint kv = ev->keyval;
    if (kv > 0x00FFu && (kv < 0xFF00u || kv > 0xFFFFu)) {
        GdkDisplay *disp = gtk_widget_get_display(w);
        GdkKeymap *km = disp ? gdk_keymap_get_for_display(disp) : NULL;
        guint base = 0;
        if (km && gdk_keymap_translate_keyboard_state(km, ev->hardware_keycode, (GdkModifierType)ev->state, 0, &base, NULL, NULL, NULL) && base != 0) {
            kv = base;
        }
    }
    if (c && c->on_key && c->on_key(c->ctx, (uint32_t)kv, (uint32_t)ev->state)) { gtk_main_quit(); return TRUE; }
    return FALSE;
}
static gboolean on_cap_key_rel(GtkWidget *w, GdkEventKey *ev, gpointer ud) {
    (void)w;
    cap_win_t *c = (cap_win_t *)ud;
    if (c && c->on_key && (ev->keyval == 0xFFE9u || ev->keyval == 0xFFEAu || ev->keyval == 0xFFE7u)) {
        c->on_key(c->ctx, (uint32_t)ev->keyval, (uint32_t)ev->state | (1u << 30));
    }
    return FALSE;
}
static gboolean on_cap_btn(GtkWidget *w, GdkEventButton *ev, gpointer ud) {
    (void)w; cap_win_t *c = (cap_win_t *)ud;
    if (!c) return FALSE;
    if (!c->fullscreen && ev->type == GDK_BUTTON_PRESS && ev->button == 1) {
        gtk_window_begin_move_drag(GTK_WINDOW(c->win), (gint)ev->button, (gint)ev->x_root, (gint)ev->y_root, ev->time);
        return TRUE;
    }
    if (!c->on_mouse) return FALSE;
    int kind = (ev->type == GDK_2BUTTON_PRESS) ? 2 : (ev->type == GDK_BUTTON_RELEASE ? 4 : 1);
    if (c->on_mouse(c->ctx, kind, (int)ev->x, (int)ev->y, 0, (int)ev->button)) gtk_main_quit();
    return TRUE;
}
static gboolean on_cap_delete(GtkWidget *w, GdkEvent *ev, gpointer ud) {
    (void)ev; (void)ud; gtk_widget_hide(w); gtk_main_quit(); return TRUE;
}
static gboolean on_cap_mot(GtkWidget *w, GdkEventMotion *ev, gpointer ud) {
    (void)w; cap_win_t *c = (cap_win_t *)ud;
    if (c && c->on_mouse) c->on_mouse(c->ctx, 5, (int)ev->x, (int)ev->y, (int)ev->state, 0);
    return FALSE;
}
static gboolean on_cap_scr(GtkWidget *w, GdkEventScroll *ev, gpointer ud) {
    (void)w; cap_win_t *c = (cap_win_t *)ud;
    int d = (ev->direction == GDK_SCROLL_UP || ev->delta_y < 0) ? 1 : -1;
    if (c && c->on_mouse) c->on_mouse(c->ctx, 6, (int)ev->x, (int)ev->y, d, 0);
    return TRUE;
}
void *tomoyo_cap_win_run(
    int w, int h, int fullscreen, int x, int y, void *ctx,
    cap_draw_cb on_draw, cap_key_cb on_key, cap_mouse_cb on_mouse
) {
    if (!ensure_gtk()) return NULL;
    cap_win_t *c = (cap_win_t *)calloc(1, sizeof(cap_win_t));
    c->ctx = ctx; c->fullscreen = fullscreen; c->on_draw = on_draw; c->on_key = on_key; c->on_mouse = on_mouse;
    c->win = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    void *wl_ovl = fullscreen ? tomoyo_cap_attach_fullscreen_overlay(c->win) : NULL;
    if (!wl_ovl) {
        GtkWidget *zero_hb = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
        gtk_widget_set_size_request(zero_hb, 0, 0);
        gtk_widget_set_no_show_all(zero_hb, TRUE);
        gtk_window_set_titlebar(GTK_WINDOW(c->win), zero_hb);
        gtk_window_set_decorated(GTK_WINDOW(c->win), FALSE);
        gtk_window_set_resizable(GTK_WINDOW(c->win), FALSE);
        gtk_window_set_keep_above(GTK_WINDOW(c->win), TRUE);
        gtk_window_set_skip_taskbar_hint(GTK_WINDOW(c->win), TRUE);
        gtk_window_set_default_size(GTK_WINDOW(c->win), w, h);
        if (fullscreen) gtk_window_fullscreen(GTK_WINDOW(c->win));
        else if (x > -30000 && y > -30000) gtk_window_move(GTK_WINDOW(c->win), x, y);
    }
    c->darea = gtk_drawing_area_new();
    gtk_widget_add_events(c->darea, GDK_BUTTON_PRESS_MASK | GDK_BUTTON_RELEASE_MASK | GDK_POINTER_MOTION_MASK | GDK_SCROLL_MASK | GDK_SMOOTH_SCROLL_MASK);
    gtk_container_add(GTK_CONTAINER(c->win), c->darea);
    g_signal_connect(c->darea, "draw", G_CALLBACK(on_cap_draw), c);
    g_signal_connect(c->darea, "button-press-event", G_CALLBACK(on_cap_btn), c);
    g_signal_connect(c->darea, "button-release-event", G_CALLBACK(on_cap_btn), c);
    g_signal_connect(c->darea, "motion-notify-event", G_CALLBACK(on_cap_mot), c);
    g_signal_connect(c->darea, "scroll-event", G_CALLBACK(on_cap_scr), c);
    g_signal_connect(c->win, "key-press-event", G_CALLBACK(on_cap_key), c);
    g_signal_connect(c->win, "key-release-event", G_CALLBACK(on_cap_key_rel), c);
    g_signal_connect(c->win, "delete-event", G_CALLBACK(on_cap_delete), NULL);
    gtk_widget_show_all(c->win);
    if (fullscreen) {
        GdkCursor *cur = gdk_cursor_new_for_display(gdk_display_get_default(), GDK_CROSSHAIR);
        if (cur) {
            if (gtk_widget_get_window(c->win)) gdk_window_set_cursor(gtk_widget_get_window(c->win), cur);
            if (gtk_widget_get_window(c->darea)) gdk_window_set_cursor(gtk_widget_get_window(c->darea), cur);
            g_object_unref(cur);
        }
    }
    gtk_main();
    gtk_widget_hide(c->win);
    tomoyo_cap_detach_fullscreen_overlay(&wl_ovl);
    gtk_widget_destroy(c->win);
    for (int i = 0; i < 12 && gtk_events_pending(); i++) gtk_main_iteration_do(FALSE);
    GdkDisplay *gd = gdk_display_get_default();
    if (gd) gdk_display_sync(gd);
    free(c);
    return NULL;
}
void tomoyo_cap_win_redraw(void) {
    GList *tops = gtk_window_list_toplevels();
    for (GList *l = tops; l; l = l->next) if (GTK_IS_WIDGET(l->data)) gtk_widget_queue_draw(GTK_WIDGET(l->data));
    g_list_free(tops);
}
static gboolean redraw_once_cb(gpointer ud) { (void)ud; tomoyo_cap_win_redraw(); return G_SOURCE_REMOVE; }
void tomoyo_cap_schedule_redraw(int ms) { g_timeout_add((guint)(ms > 0 ? ms : 50), redraw_once_cb, NULL); }
void tomoyo_cap_win_resize_top(int w, int h) {
    GList *tops = gtk_window_list_toplevels();
    if (tops && GTK_IS_WINDOW(tops->data)) gtk_window_resize(GTK_WINDOW(tops->data), w, h);
    g_list_free(tops);
}
void tomoyo_cap_win_begin_drag(int btn, int rx, int ry) {
    GList *tops = gtk_window_list_toplevels();
    if (tops && GTK_IS_WINDOW(tops->data)) gtk_window_begin_move_drag(GTK_WINDOW(tops->data), btn, rx, ry, GDK_CURRENT_TIME);
    g_list_free(tops);
}
void tomoyo_cap_win_quit(void) { gtk_main_quit(); }
void tomoyo_cap_set_cursor(int kind) {
    GList *tops = gtk_window_list_toplevels();
    for (GList *l = tops; l; l = l->next) {
        if (GTK_IS_WIDGET(l->data) && gtk_widget_get_window(GTK_WIDGET(l->data))) {
            GdkCursorType ct = GDK_CROSSHAIR;
            if (kind == 1) ct = GDK_HAND2;
            else if (kind == 2) ct = GDK_FLEUR;
            else if (kind == 3) ct = GDK_TOP_LEFT_CORNER;
            else if (kind == 4) ct = GDK_TOP_RIGHT_CORNER;
            else if (kind == 5) ct = GDK_SB_V_DOUBLE_ARROW;
            else if (kind == 6) ct = GDK_SB_H_DOUBLE_ARROW;
            GdkCursor *cur = gdk_cursor_new_for_display(gdk_display_get_default(), ct);
            if (cur) {
                gdk_window_set_cursor(gtk_widget_get_window(GTK_WIDGET(l->data)), cur);
                GtkWidget *ch = GTK_IS_BIN(l->data) ? gtk_bin_get_child(GTK_BIN(l->data)) : NULL;
                if (ch && gtk_widget_get_window(ch)) gdk_window_set_cursor(gtk_widget_get_window(ch), cur);
                g_object_unref(cur);
            }
        }
    }
    g_list_free(tops);
}

void tomoyo_cap_bake_pango_text(
    uint8_t *bgra, int w, int h, const char *font_desc, uint32_t rgb, const char *text, double x, double y
) {
    if (!bgra || w <= 0 || h <= 0 || !text || !text[0]) return;
    cairo_surface_t *s = cairo_image_surface_create_for_data(bgra, CAIRO_FORMAT_ARGB32, w, h, w * 4);
    cairo_t *cr = cairo_create(s);
    tomoyo_cap_pango_draw(cr, font_desc, rgb, 0, 0, text, x, y, (double)w - x, 64.0);
    cairo_surface_flush(s);
    cairo_destroy(cr);
    cairo_surface_destroy(s);
}

char *tomoyo_cap_prompt_text(const char *title) {
    if (!ensure_gtk()) return NULL;
    GtkWidget *d = gtk_dialog_new_with_buttons(title ? title : "输入文字", NULL, GTK_DIALOG_MODAL, "确定", GTK_RESPONSE_ACCEPT, "取消", GTK_RESPONSE_CANCEL, NULL);
    GtkWidget *ent = gtk_entry_new();
    gtk_entry_set_activates_default(GTK_ENTRY(ent), TRUE);
    gtk_box_pack_start(GTK_BOX(gtk_dialog_get_content_area(GTK_DIALOG(d))), ent, TRUE, TRUE, 8);
    gtk_dialog_set_default_response(GTK_DIALOG(d), GTK_RESPONSE_ACCEPT);
    gtk_widget_show_all(d);
    char *out = (gtk_dialog_run(GTK_DIALOG(d)) == GTK_RESPONSE_ACCEPT) ? strdup(gtk_entry_get_text(GTK_ENTRY(ent))) : NULL;
    gtk_widget_destroy(d);
    return out;
}
