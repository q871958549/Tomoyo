#ifndef TOMOYO_SHELL_INTERNAL_H
#define TOMOYO_SHELL_INTERNAL_H

#include <gtk/gtk.h>
#include <libayatana-appindicator/app-indicator.h>
#include <stdint.h>

typedef void (*tomoyo_draw_cb)(void *ctx, void *cr, int width, int height);
typedef int (*tomoyo_key_cb)(void *ctx, uint32_t keyval, uint32_t state);
typedef void (*tomoyo_mouse_cb)(void *ctx, int kind, int x, int y, int delta);
typedef void (*tomoyo_configure_cb)(void *ctx, int x, int y, int w, int h, int flags);
typedef void (*tomoyo_void_cb)(void *ctx);
typedef void (*tomoyo_cmd_cb)(void *ctx, uint32_t cmd_id);

typedef struct {
    const char *label;
    uint32_t cmd;
    int32_t parent_idx;
    int32_t checked;
    int32_t disabled;
} tomoyo_menu_item_t;

typedef struct {
    GtkWidget *window;
    GtkWidget *overlay;
    GtkWidget *darea;
    GtkWidget *fixed;
    GtkWidget *entry;
    GtkCssProvider *css;
    AppIndicator *tray;
    GtkWidget *tray_menu;
    gulong entry_changed_id;
    int suppress_change;
    gint last_btn;
    gint last_rx;
    gint last_ry;
    guint32 last_time;
    int menu_picked;
    int popup_from_kb;
    int last_w, last_h, last_ex, last_ey, last_ew, last_eh;
    int last_x, last_y, prog_x, prog_y, prog_w, prog_h, ever_shown, in_user_resize, cur_cursor;
    gint64 user_resize_until, show_until;
    void *ctx;
    tomoyo_draw_cb on_draw;
    tomoyo_key_cb on_key;
    tomoyo_mouse_cb on_mouse;
    tomoyo_configure_cb on_configure;
    tomoyo_void_cb on_text_changed;
    tomoyo_void_cb on_paste;
    tomoyo_void_cb on_focus_out;
    tomoyo_void_cb on_clip_change;
    tomoyo_cmd_cb on_tray_cmd;
} tomoyo_app_t;

#endif
