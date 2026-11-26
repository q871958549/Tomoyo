#include <gtk/gtk.h>
#include <cairo.h>
#include <pango/pangocairo.h>
#include <dlfcn.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

#if defined(__GLIBC__)
#include <malloc.h>
static void __attribute__((constructor)) tomoyo_linux_tune_glibc_malloc(void) {
    if (!getenv("MALLOC_ARENA_MAX")) {
        (void)mallopt(M_ARENA_MAX, 2);
    }
    if (!getenv("MALLOC_TRIM_THRESHOLD_")) {
        (void)mallopt(M_TRIM_THRESHOLD, 128 * 1024);
    }
    if (!getenv("MALLOC_MMAP_THRESHOLD_")) {
        (void)mallopt(M_MMAP_THRESHOLD, 128 * 1024);
    }
}
#endif

static void rounded_rect_path(cairo_t *cr, double x, double y, double w, double h, double r) {
    if (r > w * 0.5) r = w * 0.5;
    if (r > h * 0.5) r = h * 0.5;
    cairo_new_sub_path(cr);
    cairo_arc(cr, x + w - r, y + r, r, -1.5707963268, 0.0);
    cairo_arc(cr, x + w - r, y + h - r, r, 0.0, 1.5707963268);
    cairo_arc(cr, x + r, y + h - r, r, 1.5707963268, 3.1415926536);
    cairo_arc(cr, x + r, y + r, r, 3.1415926536, 4.7123889804);
    cairo_close_path(cr);
}

void tomoyo_cairo_begin_rounded(void *cr_v, double w, double h, double r) {
    cairo_t *cr = (cairo_t *)cr_v;
    if (!cr || w <= 0 || h <= 0) return;
    cairo_save(cr);
    cairo_set_operator(cr, CAIRO_OPERATOR_SOURCE);
    cairo_set_source_rgba(cr, 0.0, 0.0, 0.0, 0.0);
    cairo_paint(cr);
    cairo_restore(cr);
    cairo_save(cr);
    rounded_rect_path(cr, 0.0, 0.0, w, h, r);
    cairo_clip(cr);
}

void tomoyo_cairo_end_rounded(void *cr_v, double w, double h, double r, uint32_t border_rgb) {
    cairo_t *cr = (cairo_t *)cr_v;
    if (!cr || w <= 0 || h <= 0) return;
    cairo_restore(cr);
    cairo_save(cr);
    rounded_rect_path(cr, 0.5, 0.5, w - 1.0, h - 1.0, r);
    cairo_set_source_rgba(
        cr,
        ((border_rgb >> 16) & 0xff) / 255.0,
        ((border_rgb >> 8) & 0xff) / 255.0,
        (border_rgb & 0xff) / 255.0,
        0.85
    );
    cairo_set_line_width(cr, 1.0);
    cairo_stroke(cr);
    cairo_restore(cr);
}

static inline double snap_px(double v) {
    return (double)(long long)(v >= 0.0 ? (v + 0.5) : (v - 0.5));
}

double tomoyo_cairo_target_scale(void *cr_v) {
    cairo_t *cr = (cairo_t *)cr_v;
    if (!cr) return 1.0;
    double sx = 1.0, sy = 1.0;
    cairo_surface_t *t = cairo_get_target(cr);
    if (t) cairo_surface_get_device_scale(t, &sx, &sy);
    return sx > sy ? sx : sy;
}

void *tomoyo_opaque_cairo_begin(uint8_t *bgra, int w, int h) {
    if (!bgra || w <= 0 || h <= 0) return NULL;
    cairo_surface_t *s = cairo_image_surface_create_for_data(bgra, CAIRO_FORMAT_RGB24, w, h, w * 4);
    if (cairo_surface_status(s) != CAIRO_STATUS_SUCCESS) {
        cairo_surface_destroy(s);
        return NULL;
    }
    cairo_t *cr = cairo_create(s);
    cairo_surface_destroy(s);
    GdkScreen *scr = gdk_display_get_default() ? gdk_screen_get_default() : NULL;
    const cairo_font_options_t *sfo = scr ? gdk_screen_get_font_options(scr) : NULL;
    cairo_font_options_t *fo = sfo ? cairo_font_options_copy(sfo) : cairo_font_options_create();
    if (fo) {
        if (cairo_font_options_get_antialias(fo) != CAIRO_ANTIALIAS_NONE) {
            cairo_font_options_set_antialias(fo, CAIRO_ANTIALIAS_SUBPIXEL);
        }
        if (cairo_font_options_get_subpixel_order(fo) == CAIRO_SUBPIXEL_ORDER_DEFAULT) {
            cairo_font_options_set_subpixel_order(fo, CAIRO_SUBPIXEL_ORDER_RGB);
        }
        if (cairo_font_options_get_hint_style(fo) == CAIRO_HINT_STYLE_DEFAULT) {
            cairo_font_options_set_hint_style(fo, CAIRO_HINT_STYLE_SLIGHT);
        }
        cairo_font_options_set_hint_metrics(fo, CAIRO_HINT_METRICS_ON);
        cairo_set_font_options(cr, fo);
        cairo_font_options_destroy(fo);
    }
    return cr;
}

void tomoyo_opaque_cairo_end(void *cr_v) {
    cairo_t *cr = (cairo_t *)cr_v;
    if (!cr) return;
    cairo_surface_flush(cairo_get_target(cr));
    cairo_destroy(cr);
}

void tomoyo_cairo_blit_rgb24(void *cr_v, const uint8_t *bgra, int w, int h, double x, double y, double dw, double dh) {
    cairo_t *cr = (cairo_t *)cr_v;
    if (!cr || !bgra || w <= 0 || h <= 0 || dw <= 0 || dh <= 0) return;
    cairo_surface_t *s = cairo_image_surface_create_for_data((unsigned char *)bgra, CAIRO_FORMAT_RGB24, w, h, w * 4);
    cairo_save(cr);
    cairo_translate(cr, x, y);
    cairo_scale(cr, dw / (double)w, dh / (double)h);
    cairo_set_source_surface(cr, s, 0, 0);
    cairo_paint(cr);
    cairo_restore(cr);
    cairo_surface_destroy(s);
}

void tomoyo_cairo_blit_bgra(void *cr_v, const uint8_t *bgra, int w, int h, double x, double y, double dw, double dh) {
    cairo_t *cr = (cairo_t *)cr_v;
    if (!cr || !bgra || w <= 0 || h <= 0 || dw <= 0 || dh <= 0) return;
    cairo_surface_t *s = cairo_image_surface_create_for_data((unsigned char *)bgra, CAIRO_FORMAT_ARGB32, w, h, w * 4);
    cairo_save(cr);
    cairo_translate(cr, x, y);
    cairo_scale(cr, dw / (double)w, dh / (double)h);
    cairo_set_source_surface(cr, s, 0, 0);
    cairo_paint(cr);
    cairo_restore(cr);
    cairo_surface_destroy(s);
}

void tomoyo_pango_draw_text(
    void *cr_v, const char *family, int weight, double size_px, int tabular_nums,
    uint32_t rgb, int align, int single_line,
    const char *text, double x, double y, double w, double h,
    const uint32_t *hl_pairs, size_t hl_count, uint32_t hl_rgb
) {
    cairo_t *cr = (cairo_t *)cr_v;
    if (!cr || !text || !text[0] || w <= 2.0 || h <= 2.0) return;
    PangoLayout *ly = pango_cairo_create_layout(cr);
    PangoFontDescription *desc = pango_font_description_new();
    pango_font_description_set_family(desc, (family && family[0]) ? family : "sans-serif");
    pango_font_description_set_weight(desc, (PangoWeight)(weight > 0 ? weight : 400));
    pango_font_description_set_absolute_size(desc, (size_px > 4.0 ? size_px : 12.0) * (double)PANGO_SCALE);
    pango_layout_set_font_description(ly, desc);
    pango_layout_set_text(ly, text, -1);
    pango_layout_set_width(ly, (int)(w * PANGO_SCALE));
    pango_layout_set_alignment(
        ly,
        align == 1 ? PANGO_ALIGN_CENTER : (align == 2 ? PANGO_ALIGN_RIGHT : PANGO_ALIGN_LEFT)
    );
    if (single_line) {
        pango_layout_set_ellipsize(ly, PANGO_ELLIPSIZE_END);
    } else {
        pango_layout_set_wrap(ly, PANGO_WRAP_WORD_CHAR);
        pango_layout_set_height(ly, (int)(h * PANGO_SCALE));
    }

    if (tabular_nums || (hl_pairs && hl_count > 0)) {
        PangoAttrList *al = pango_attr_list_new();
#if PANGO_VERSION_CHECK(1, 38, 0)
        if (tabular_nums) {
            pango_attr_list_insert(al, pango_attr_font_features_new("tnum=1"));
        }
#endif
        if (hl_pairs && hl_count > 0) {
            for (size_t i = 0; i < hl_count; i++) {
                PangoAttribute *at = pango_attr_foreground_new(
                    ((hl_rgb >> 16) & 0xff) * 257,
                    ((hl_rgb >> 8) & 0xff) * 257,
                    (hl_rgb & 0xff) * 257
                );
                at->start_index = hl_pairs[i * 2];
                at->end_index = hl_pairs[i * 2 + 1];
                pango_attr_list_insert(al, at);
            }
        }
        pango_layout_set_attributes(ly, al);
        pango_attr_list_unref(al);
    }

    pango_cairo_update_layout(cr, ly);
    int pw = 0, ph = 0;
    pango_layout_get_pixel_size(ly, &pw, &ph);
    double dx = snap_px(x);
    double dy = snap_px(single_line ? (y + (h - (double)ph) * 0.5) : y);
    cairo_save(cr);
    cairo_rectangle(cr, x, y, w, h);
    cairo_clip(cr);
    cairo_set_source_rgb(cr, ((rgb >> 16) & 0xff) / 255.0, ((rgb >> 8) & 0xff) / 255.0, (rgb & 0xff) / 255.0);
    cairo_move_to(cr, dx, dy);
    pango_cairo_show_layout(cr, ly);
    cairo_restore(cr);
    pango_font_description_free(desc);
    g_object_unref(ly);
}

int tomoyo_linux_resolve_icon_path(const char *icon_name, int sz, char *out, size_t out_len) {
    if (!icon_name || !icon_name[0] || !out || out_len < 2 || gdk_screen_get_default() == NULL) return 0;
    out[0] = '\0';
    GtkIconTheme *th = gtk_icon_theme_get_default();
    if (!th) return 0;
    GtkIconInfo *info = gtk_icon_theme_lookup_icon(th, icon_name, sz, GTK_ICON_LOOKUP_NO_SVG);
    if (!info) info = gtk_icon_theme_lookup_icon(th, icon_name, sz, 0);
    if (!info) return 0;
    const gchar *fn = gtk_icon_info_get_filename(info);
    int ok = 0;
    if (fn && fn[0] && strlen(fn) < out_len) {
        snprintf(out, out_len, "%s", fn);
        ok = 1;
    }
    g_object_unref(info);
    return ok;
}

typedef struct {
    double x;
    double y;
    double width;
    double height;
} tomoyo_rsvg_rect_t;

typedef void *(*rsvg_new_from_file_fn)(const char *filename, GError **error);
typedef gboolean (*rsvg_render_doc_fn)(void *handle, cairo_t *cr, const tomoyo_rsvg_rect_t *viewport, GError **error);

static int has_svg_suffix(const char *path) {
    if (!path) return 0;
    size_t n = strlen(path);
    return (n >= 4 && strcasecmp(path + n - 4, ".svg") == 0)
        || (n >= 5 && strcasecmp(path + n - 5, ".svgz") == 0);
}

static int tomoyo_render_svg_bgra(const char *path, int sz, uint8_t *out, size_t out_len) {
    static gsize rsvg_once = 0;
    static rsvg_new_from_file_fn fn_new = NULL;
    static rsvg_render_doc_fn fn_render = NULL;
    if (g_once_init_enter(&rsvg_once)) {
        void *lib = dlopen("librsvg-2.so.2", RTLD_LAZY | RTLD_LOCAL);
        if (!lib) lib = dlopen("librsvg-2.so", RTLD_LAZY | RTLD_LOCAL);
        if (lib) {
            void *s_new = dlsym(lib, "rsvg_handle_new_from_file");
            void *s_ren = dlsym(lib, "rsvg_handle_render_document");
            if (s_new && s_ren) {
                memcpy(&fn_new, &s_new, sizeof(fn_new));
                memcpy(&fn_render, &s_ren, sizeof(fn_render));
            }
        }
        g_once_init_leave(&rsvg_once, 1);
    }
    if (!fn_new || !fn_render || !path || sz <= 0 || out_len < (size_t)sz * (size_t)sz * 4) return 0;
    void *handle = fn_new(path, NULL);
    if (!handle) return 0;
    memset(out, 0, (size_t)sz * (size_t)sz * 4);
    cairo_surface_t *surf = cairo_image_surface_create_for_data(out, CAIRO_FORMAT_ARGB32, sz, sz, sz * 4);
    if (cairo_surface_status(surf) != CAIRO_STATUS_SUCCESS) {
        cairo_surface_destroy(surf);
        g_object_unref(handle);
        return 0;
    }
    cairo_t *cr = cairo_create(surf);
    tomoyo_rsvg_rect_t vp = { 0.0, 0.0, (double)sz, (double)sz };
    gboolean ok = fn_render(handle, cr, &vp, NULL);
    cairo_surface_flush(surf);
    cairo_destroy(cr);
    cairo_surface_destroy(surf);
    g_object_unref(handle);
    return ok ? 1 : 0;
}

int tomoyo_linux_load_icon_bgra(const char *icon_name_or_path, int sz, uint8_t *out, size_t out_len) {
    if (!icon_name_or_path || !icon_name_or_path[0] || !out || sz <= 0 || out_len < (size_t)(sz * sz * 4)) return 0;
    char resolved[512];
    const char *path = NULL;
    if (icon_name_or_path[0] == '/') {
        path = icon_name_or_path;
    } else if (tomoyo_linux_resolve_icon_path(icon_name_or_path, sz, resolved, sizeof(resolved))) {
        path = resolved;
    }
    if (path && has_svg_suffix(path) && tomoyo_render_svg_bgra(path, sz, out, out_len)) {
        return 1;
    }
    GdkPixbuf *pb = NULL;
    if (path) {
        pb = gdk_pixbuf_new_from_file_at_scale(path, sz, sz, TRUE, NULL);
    } else if (gdk_screen_get_default() != NULL) {
        GtkIconTheme *th = gtk_icon_theme_get_default();
        if (th) pb = gtk_icon_theme_load_icon(th, icon_name_or_path, sz, GTK_ICON_LOOKUP_FORCE_SIZE, NULL);
    }
    if (!pb) return 0;
    if (gdk_pixbuf_get_width(pb) != sz || gdk_pixbuf_get_height(pb) != sz) {
        GdkPixbuf *sc = gdk_pixbuf_scale_simple(pb, sz, sz, GDK_INTERP_BILINEAR);
        g_object_unref(pb);
        pb = sc;
        if (!pb) return 0;
    }
    int nc = gdk_pixbuf_get_n_channels(pb), rs = gdk_pixbuf_get_rowstride(pb);
    const guchar *px = gdk_pixbuf_get_pixels(pb);
    for (int y = 0; y < sz; y++) {
        for (int x = 0; x < sz; x++) {
            const guchar *s = px + y * rs + x * nc;
            uint8_t r = s[0], g = s[1], b = s[2], al = nc >= 4 ? s[3] : 255;
            size_t di = ((size_t)y * sz + x) * 4;
            out[di] = (uint8_t)((b * al + 127) / 255);
            out[di + 1] = (uint8_t)((g * al + 127) / 255);
            out[di + 2] = (uint8_t)((r * al + 127) / 255);
            out[di + 3] = al;
        }
    }
    g_object_unref(pb);
    return 1;
}

void *tomoyo_offscreen_cairo_create(int w, int h, uint8_t *bgra) {
    cairo_surface_t *s = cairo_image_surface_create_for_data(bgra, CAIRO_FORMAT_ARGB32, w, h, w * 4);
    cairo_t *cr = cairo_create(s);
    cairo_surface_destroy(s);
    return cr;
}

void tomoyo_offscreen_cairo_destroy(void *cr) {
    if (cr) cairo_destroy((cairo_t *)cr);
}
