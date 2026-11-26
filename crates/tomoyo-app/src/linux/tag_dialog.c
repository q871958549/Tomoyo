#include "shell_internal.h"
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    GtkWidget *dlg, *e_label, *e_exts, *e_expr, *lbl_preview, *lbl_err;
    GtkWidget *cond_btns[5];
    int is_en, updating;
} tag_dlg_t;

static const char *PRESETS[4][3] = {
    {"代码", "Code", "rs, ts, tsx, js, py, go, c, cpp, h, toml, json"},
    {"视频", "Video", "mp4, mkv, mov, avi, webm, flv"},
    {"音频", "Audio", "mp3, flac, wav, m4a, ogg, aac"},
    {"设计", "Design", "psd, fig, ai, sketch, xd, svg"},
};
static const char *CONDS[5][3] = {
    {"今日修改", "Today", "dm:today"},
    {"近7天", "7 Days", "dm:last7days"},
    {"大文件", ">50MB", "size:>50mb"},
    {"仅文件夹", "Folders", "folder:"},
    {"下载目录", "Downloads", "path:Downloads"},
};

static int has_token_ci(const char *hay, const char *tok) {
    if (!hay || !tok || !*tok) return 0;
    size_t tl = strlen(tok);
    const char *p = hay;
    while (*p) {
        while (*p && isspace((unsigned char)*p)) p++;
        const char *start = p;
        while (*p && !isspace((unsigned char)*p)) p++;
        if ((size_t)(p - start) == tl && g_ascii_strncasecmp(start, tok, tl) == 0) return 1;
    }
    return 0;
}

static void toggle_expr_tok(const char *src, const char *tok, char *dst, size_t dst_sz) {
    int present = has_token_ci(src, tok);
    size_t tl = strlen(tok);
    dst[0] = '\0';
    const char *p = src ? src : "";
    while (*p) {
        while (*p && isspace((unsigned char)*p)) p++;
        const char *s = p;
        while (*p && !isspace((unsigned char)*p)) p++;
        size_t len = (size_t)(p - s);
        if (len == 0) continue;
        if (present && len == tl && g_ascii_strncasecmp(s, tok, tl) == 0) continue;
        if (dst[0]) g_strlcat(dst, " ", dst_sz);
        size_t cur = strlen(dst);
        if (cur + len + 1 < dst_sz) { memcpy(dst + cur, s, len); dst[cur + len] = '\0'; }
    }
    if (!present) {
        if (dst[0]) g_strlcat(dst, " ", dst_sz);
        g_strlcat(dst, tok, dst_sz);
    }
}

static void sync_preview_and_chips(tag_dlg_t *d) {
    const char *ex = gtk_entry_get_text(GTK_ENTRY(d->e_exts));
    const char *xp = gtk_entry_get_text(GTK_ENTRY(d->e_expr));
    d->updating = 1;
    for (int i = 0; i < 5; i++) {
        gtk_toggle_button_set_active(GTK_TOGGLE_BUTTON(d->cond_btns[i]), has_token_ci(xp, CONDS[i][2]) ? TRUE : FALSE);
    }
    d->updating = 0;
    char *tx = g_strdup(ex ? ex : ""), *tp = g_strdup(xp ? xp : "");
    g_strstrip(tx); g_strstrip(tp);
    char buf[512];
    if (*tx && *tp) snprintf(buf, sizeof(buf), "%s %s  |  %s", d->is_en ? "Match:" : "规则预览:", tx, tp);
    else if (*tx) snprintf(buf, sizeof(buf), "%s %s", d->is_en ? "Extensions:" : "匹配扩展名:", tx);
    else if (*tp) snprintf(buf, sizeof(buf), "%s %s", d->is_en ? "Condition:" : "条件筛选:", tp);
    else snprintf(buf, sizeof(buf), "%s", d->is_en ? "Add extensions or click a quick rule above" : "填写扩展名或点击上方快捷规则组合条件");
    gtk_label_set_text(GTK_LABEL(d->lbl_preview), buf);
    gtk_label_set_text(GTK_LABEL(d->lbl_err), "");
    g_free(tx); g_free(tp);
}

static void on_field_changed(GtkEditable *ed, gpointer ud) {
    (void)ed;
    tag_dlg_t *d = (tag_dlg_t *)ud;
    if (!d->updating) sync_preview_and_chips(d);
}

static void on_preset_clicked(GtkButton *btn, gpointer ud) {
    tag_dlg_t *d = (tag_dlg_t *)ud;
    int idx = GPOINTER_TO_INT(g_object_get_data(G_OBJECT(btn), "pidx"));
    if (idx < 0 || idx >= 4) return;
    const char *cur_lbl = gtk_entry_get_text(GTK_ENTRY(d->e_label));
    if (!cur_lbl || !*cur_lbl) gtk_entry_set_text(GTK_ENTRY(d->e_label), PRESETS[idx][d->is_en ? 1 : 0]);
    gtk_entry_set_text(GTK_ENTRY(d->e_exts), PRESETS[idx][2]);
}

static void on_cond_toggled(GtkToggleButton *tb, gpointer ud) {
    (void)tb;
    tag_dlg_t *d = (tag_dlg_t *)ud;
    if (d->updating) return;
    int idx = GPOINTER_TO_INT(g_object_get_data(G_OBJECT(tb), "cidx"));
    if (idx < 0 || idx >= 5) return;
    char next[512];
    toggle_expr_tok(gtk_entry_get_text(GTK_ENTRY(d->e_expr)), CONDS[idx][2], next, sizeof(next));
    gtk_entry_set_text(GTK_ENTRY(d->e_expr), next);
}

static void try_submit(GtkWidget *w, gpointer ud) {
    (void)w;
    tag_dlg_t *d = (tag_dlg_t *)ud;
    char *l = g_strdup(gtk_entry_get_text(GTK_ENTRY(d->e_label)));
    char *e = g_strdup(gtk_entry_get_text(GTK_ENTRY(d->e_exts)));
    char *x = g_strdup(gtk_entry_get_text(GTK_ENTRY(d->e_expr)));
    g_strstrip(l); g_strstrip(e); g_strstrip(x);
    if (!*l) {
        gtk_label_set_text(GTK_LABEL(d->lbl_err), d->is_en ? "Tag name cannot be empty." : "标签名称不能为空");
        gtk_widget_grab_focus(d->e_label);
    } else if (!*e && !*x) {
        gtk_label_set_text(GTK_LABEL(d->lbl_err), d->is_en ? "Enter at least one extension or condition." : "至少填写扩展名或一个条件");
        gtk_widget_grab_focus(d->e_exts);
    } else {
        gtk_dialog_response(GTK_DIALOG(d->dlg), GTK_RESPONSE_OK);
    }
    g_free(l); g_free(e); g_free(x);
}

static GtkWidget *make_entry(tag_dlg_t *d, const char *val, const char *ph) {
    GtkWidget *e = gtk_entry_new();
    gtk_entry_set_text(GTK_ENTRY(e), val ? val : "");
    gtk_entry_set_placeholder_text(GTK_ENTRY(e), ph);
    g_signal_connect(e, "changed", G_CALLBACK(on_field_changed), d);
    g_signal_connect(e, "activate", G_CALLBACK(try_submit), d);
    return e;
}

int tomoyo_linux_show_tag_dialog(
    tomoyo_app_t *a, int is_edit, int theme_mode, int is_en,
    const char *init_label, const char *init_exts, const char *init_expr,
    char *out_label, size_t label_sz, char *out_exts, size_t exts_sz, char *out_expr, size_t expr_sz
) {
    tag_dlg_t d = {0};
    d.is_en = is_en ? 1 : 0;
    const char *title = is_edit
        ? (d.is_en ? "Edit Custom Tag" : "编辑自定义标签")
        : (d.is_en ? "New Custom Tag" : "新建自定义标签");
    GtkWindow *parent = (a && a->window) ? GTK_WINDOW(a->window) : NULL;
    d.dlg = gtk_dialog_new_with_buttons(
        title, parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT,
        d.is_en ? "Cancel" : "取消", GTK_RESPONSE_CANCEL, NULL
    );
    gtk_window_set_default_size(GTK_WINDOW(d.dlg), 440, 340);
    gtk_window_set_position(GTK_WINDOW(d.dlg), parent ? GTK_WIN_POS_CENTER_ON_PARENT : GTK_WIN_POS_CENTER);
    GtkWidget *btn_save = gtk_dialog_add_button(GTK_DIALOG(d.dlg), d.is_en ? "Save Tag" : "保存标签", 99);
    gtk_style_context_add_class(gtk_widget_get_style_context(btn_save), "suggested-action");
    g_signal_connect(btn_save, "clicked", G_CALLBACK(try_submit), &d);

    const char *bg = theme_mode == 0 ? "#f7f8fa" : (theme_mode == 2 ? "#0f1320" : "#181920");
    const char *fg = theme_mode == 0 ? "#111827" : "#f3f4f6";
    const char *card = theme_mode == 0 ? "#ffffff" : (theme_mode == 2 ? "#161c2e" : "#22242e");
    const char *bd = theme_mode == 0 ? "#dce0ea" : (theme_mode == 2 ? "#26304c" : "#313545");
    char css[768];
    snprintf(css, sizeof(css),
        "dialog#tomoyo-tag-dlg { background: %s; color: %s; } "
        "dialog#tomoyo-tag-dlg entry { background: %s; color: %s; border: 1px solid %s; border-radius: 6px; padding: 6px 8px; } "
        "dialog#tomoyo-tag-dlg button.chip-btn { padding: 3px 9px; border-radius: 6px; font-size: 12px; } "
        "dialog#tomoyo-tag-dlg label.preview-lbl { opacity: 0.78; font-size: 11.5px; } "
        "dialog#tomoyo-tag-dlg label.err-lbl { color: #ef4444; font-size: 11.5px; }",
        bg, fg, card, fg, bd);
    gtk_widget_set_name(d.dlg, "tomoyo-tag-dlg");
    GtkCssProvider *cp = gtk_css_provider_new();
    gtk_css_provider_load_from_data(cp, css, -1, NULL);
    gtk_style_context_add_provider_for_screen(gdk_screen_get_default(), GTK_STYLE_PROVIDER(cp), GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);

    GtkWidget *box = gtk_dialog_get_content_area(GTK_DIALOG(d.dlg));
    gtk_box_set_spacing(GTK_BOX(box), 8);
    gtk_container_set_border_width(GTK_CONTAINER(box), 14);

    d.e_label = make_entry(&d, init_label, d.is_en ? "e.g. Code, Videos, Reports" : "例如：代码、视频、合同");
    d.e_exts = make_entry(&d, init_exts, d.is_en ? "e.g. rs, ts, py (comma separated)" : "例如：rs, ts, py（逗号分隔，可留空）");
    d.e_expr = make_entry(&d, init_expr, d.is_en ? "e.g. dm:today size:>10mb path:Downloads" : "例如：dm:today size:>10mb path:Downloads");

    gtk_box_pack_start(GTK_BOX(box), gtk_label_new(d.is_en ? "Tag Name" : "标签名称"), FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(box), d.e_label, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(box), gtk_label_new(d.is_en ? "Extensions & Presets" : "文件扩展名与常用预设"), FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(box), d.e_exts, FALSE, FALSE, 0);

    GtkWidget *prow = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 6);
    for (int i = 0; i < 4; i++) {
        GtkWidget *b = gtk_button_new_with_label(PRESETS[i][d.is_en ? 1 : 0]);
        gtk_style_context_add_class(gtk_widget_get_style_context(b), "chip-btn");
        g_object_set_data(G_OBJECT(b), "pidx", GINT_TO_POINTER(i));
        g_signal_connect(b, "clicked", G_CALLBACK(on_preset_clicked), &d);
        gtk_box_pack_start(GTK_BOX(prow), b, FALSE, FALSE, 0);
    }
    gtk_box_pack_start(GTK_BOX(box), prow, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(box), gtk_label_new(d.is_en ? "Conditions & Natural Rules" : "时间 / 大小 / 位置组合条件"), FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(box), d.e_expr, FALSE, FALSE, 0);

    GtkWidget *crow = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 6);
    for (int i = 0; i < 5; i++) {
        d.cond_btns[i] = gtk_toggle_button_new_with_label(CONDS[i][d.is_en ? 1 : 0]);
        gtk_style_context_add_class(gtk_widget_get_style_context(d.cond_btns[i]), "chip-btn");
        g_object_set_data(G_OBJECT(d.cond_btns[i]), "cidx", GINT_TO_POINTER(i));
        g_signal_connect(d.cond_btns[i], "toggled", G_CALLBACK(on_cond_toggled), &d);
        gtk_box_pack_start(GTK_BOX(crow), d.cond_btns[i], FALSE, FALSE, 0);
    }
    gtk_box_pack_start(GTK_BOX(box), crow, FALSE, FALSE, 0);

    d.lbl_preview = gtk_label_new("");
    gtk_label_set_xalign(GTK_LABEL(d.lbl_preview), 0.0f);
    gtk_style_context_add_class(gtk_widget_get_style_context(d.lbl_preview), "preview-lbl");
    d.lbl_err = gtk_label_new("");
    gtk_label_set_xalign(GTK_LABEL(d.lbl_err), 0.0f);
    gtk_style_context_add_class(gtk_widget_get_style_context(d.lbl_err), "err-lbl");
    gtk_box_pack_start(GTK_BOX(box), d.lbl_preview, FALSE, FALSE, 2);
    gtk_box_pack_start(GTK_BOX(box), d.lbl_err, FALSE, FALSE, 0);

    sync_preview_and_chips(&d);
    gtk_widget_show_all(d.dlg);
    int resp = gtk_dialog_run(GTK_DIALOG(d.dlg));
    int ok = 0;
    if (resp == GTK_RESPONSE_OK) {
        if (out_label && label_sz > 0) g_strlcpy(out_label, gtk_entry_get_text(GTK_ENTRY(d.e_label)), label_sz);
        if (out_exts && exts_sz > 0) g_strlcpy(out_exts, gtk_entry_get_text(GTK_ENTRY(d.e_exts)), exts_sz);
        if (out_expr && expr_sz > 0) g_strlcpy(out_expr, gtk_entry_get_text(GTK_ENTRY(d.e_expr)), expr_sz);
        ok = 1;
    }
    gtk_style_context_remove_provider_for_screen(gdk_screen_get_default(), GTK_STYLE_PROVIDER(cp));
    g_object_unref(cp);
    gtk_widget_destroy(d.dlg);
    return ok;
}
