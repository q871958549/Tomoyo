#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef HAVE_LIBCURL
#include <curl/curl.h>
#endif
#ifdef HAVE_FONTCONFIG
#include <fontconfig/fontconfig.h>
#endif
#ifdef HAVE_GDK_PIXBUF_2_0
#include <gdk-pixbuf/gdk-pixbuf.h>
#endif
#ifdef HAVE_LIBSECRET_1
#include <libsecret/secret.h>
#endif

typedef size_t (*tomoyo_bytes_cb)(void *ctx, const uint8_t *data, size_t len);
typedef int (*tomoyo_prog_cb)(void *ctx, uint64_t dl_now, uint64_t dl_total);
typedef void (*tomoyo_str_cb)(void *ctx, const char *s, size_t len);

#ifdef HAVE_LIBCURL
typedef struct {
    tomoyo_bytes_cb body_cb;
    tomoyo_bytes_cb hdr_cb;
    tomoyo_prog_cb prog_cb;
    void *ctx;
} curl_ctx_t;

static size_t on_curl_write(char *ptr, size_t size, size_t nmemb, void *userdata) {
    curl_ctx_t *c = (curl_ctx_t *)userdata;
    size_t n = size * nmemb;
    if (!c || !c->body_cb) return n;
    return c->body_cb(c->ctx, (const uint8_t *)ptr, n);
}

static size_t on_curl_header(char *buffer, size_t size, size_t nitems, void *userdata) {
    curl_ctx_t *c = (curl_ctx_t *)userdata;
    size_t n = size * nitems;
    if (c && c->hdr_cb) c->hdr_cb(c->ctx, (const uint8_t *)buffer, n);
    return n;
}

static int on_curl_xfer(void *clientp, curl_off_t dltotal, curl_off_t dlnow,
                        curl_off_t ultotal, curl_off_t ulnow) {
    (void)ultotal;
    (void)ulnow;
    curl_ctx_t *c = (curl_ctx_t *)clientp;
    if (!c || !c->prog_cb) return 0;
    uint64_t now_u = dlnow > 0 ? (uint64_t)dlnow : 0;
    uint64_t tot_u = dltotal > 0 ? (uint64_t)dltotal : 0;
    return c->prog_cb(c->ctx, now_u, tot_u);
}

int tomoyo_curl_perform(
    const char *method,
    const char *url,
    const char *headers_nl,
    const uint8_t *post_body,
    size_t post_len,
    uint32_t timeout_ms,
    int follow_redirects,
    int nobody,
    tomoyo_bytes_cb body_cb,
    tomoyo_bytes_cb hdr_cb,
    tomoyo_prog_cb prog_cb,
    void *ctx,
    uint16_t *out_status,
    tomoyo_str_cb final_url_cb,
    char *err_buf,
    size_t err_buf_len
) {
    CURL *curl = curl_easy_init();
    if (!curl) {
        if (err_buf && err_buf_len > 0) snprintf(err_buf, err_buf_len, "curl_easy_init failed");
        return -1;
    }
    curl_ctx_t cctx = { body_cb, hdr_cb, prog_cb, ctx };
    struct curl_slist *slist = NULL;

    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_USERAGENT, "Tomoyo/2.3 (Linux)");
    curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT_MS, (long)(timeout_ms > 0 ? timeout_ms : 8000));
    curl_easy_setopt(curl, CURLOPT_TIMEOUT_MS, (long)(timeout_ms > 0 ? timeout_ms : 15000));
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, follow_redirects ? 1L : 0L);
    curl_easy_setopt(curl, CURLOPT_MAXREDIRS, 8L);
    curl_easy_setopt(curl, CURLOPT_NOSIGNAL, 1L);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, on_curl_write);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &cctx);
    curl_easy_setopt(curl, CURLOPT_HEADERFUNCTION, on_curl_header);
    curl_easy_setopt(curl, CURLOPT_HEADERDATA, &cctx);
    if (prog_cb) {
        curl_easy_setopt(curl, CURLOPT_XFERINFOFUNCTION, on_curl_xfer);
        curl_easy_setopt(curl, CURLOPT_XFERINFODATA, &cctx);
        curl_easy_setopt(curl, CURLOPT_NOPROGRESS, 0L);
    }
    if (nobody) {
        curl_easy_setopt(curl, CURLOPT_NOBODY, 1L);
    } else if (method && strcmp(method, "POST") == 0) {
        curl_easy_setopt(curl, CURLOPT_POST, 1L);
        curl_easy_setopt(curl, CURLOPT_POSTFIELDS, post_body ? (const void *)post_body : (const void *)"");
        curl_easy_setopt(curl, CURLOPT_POSTFIELDSIZE_LARGE, (curl_off_t)post_len);
    } else if (method && strcmp(method, "GET") != 0) {
        curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, method);
    }

    if (headers_nl && headers_nl[0] != '\0') {
        const char *p = headers_nl;
        while (*p) {
            const char *nl = strchr(p, '\n');
            size_t len = nl ? (size_t)(nl - p) : strlen(p);
            if (len > 0) {
                char *line = (char *)malloc(len + 1);
                if (line) {
                    memcpy(line, p, len);
                    line[len] = '\0';
                    slist = curl_slist_append(slist, line);
                    free(line);
                }
            }
            if (!nl) break;
            p = nl + 1;
        }
        if (slist) curl_easy_setopt(curl, CURLOPT_HTTPHEADER, slist);
    }

    CURLcode res = curl_easy_perform(curl);
    if (res != CURLE_OK) {
        if (err_buf && err_buf_len > 0) {
            snprintf(err_buf, err_buf_len, "%s", curl_easy_strerror(res));
        }
        if (slist) curl_slist_free_all(slist);
        curl_easy_cleanup(curl);
        return (int)res;
    }

    long code = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &code);
    if (out_status) *out_status = (uint16_t)code;

    char *eff = NULL;
    if (final_url_cb && curl_easy_getinfo(curl, CURLINFO_EFFECTIVE_URL, &eff) == CURLE_OK && eff) {
        final_url_cb(ctx, eff, strlen(eff));
    }

    if (slist) curl_slist_free_all(slist);
    curl_easy_cleanup(curl);
    return 0;
}
#else
int tomoyo_curl_perform(
    const char *m, const char *u, const char *h, const uint8_t *b, size_t bl,
    uint32_t t, int fr, int nb, tomoyo_bytes_cb bc, tomoyo_bytes_cb hc,
    tomoyo_prog_cb pc, void *ctx, uint16_t *st, tomoyo_str_cb fc, char *eb, size_t el
) {
    (void)m; (void)u; (void)h; (void)b; (void)bl; (void)t; (void)fr; (void)nb;
    (void)bc; (void)hc; (void)pc; (void)ctx; (void)st; (void)fc;
    if (eb && el > 0) snprintf(eb, el, "libcurl development headers missing at build time");
    return -1;
}
#endif

int tomoyo_fc_add_font(const char *path) {
#ifdef HAVE_FONTCONFIG
    if (!path || !FcInit()) return 0;
    return FcConfigAppFontAddFile(NULL, (const FcChar8 *)path) ? 1 : 0;
#else
    (void)path;
    return 0;
#endif
}

void tomoyo_fc_clear_fonts(void) {
#ifdef HAVE_FONTCONFIG
    FcConfigAppFontClear(NULL);
#endif
}

int tomoyo_decode_image_rgba(
    const char *path,
    int max_w,
    int max_h,
    uint32_t *out_src_w,
    uint32_t *out_src_h,
    uint32_t *out_dst_w,
    uint32_t *out_dst_h,
    tomoyo_bytes_cb rgba_cb,
    void *ctx
) {
#ifdef HAVE_GDK_PIXBUF_2_0
    if (!path || !rgba_cb) return 0;
    int sw = 0, sh = 0;
    if (!gdk_pixbuf_get_file_info(path, &sw, &sh) || sw <= 0 || sh <= 0) return 0;
    int tw = (sw > max_w) ? max_w : sw;
    int th = (sh > max_h) ? max_h : sh;
    if (tw < 1) tw = 1;
    if (th < 1) th = 1;

    GError *err = NULL;
    GdkPixbuf *pb = gdk_pixbuf_new_from_file_at_scale(path, tw, th, TRUE, &err);
    if (!pb) {
        if (err) g_error_free(err);
        return 0;
    }
    if (!gdk_pixbuf_get_has_alpha(pb)) {
        GdkPixbuf *alpha = gdk_pixbuf_add_alpha(pb, FALSE, 0, 0, 0);
        g_object_unref(pb);
        if (!alpha) return 0;
        pb = alpha;
    }
    int dw = gdk_pixbuf_get_width(pb);
    int dh = gdk_pixbuf_get_height(pb);
    int stride = gdk_pixbuf_get_rowstride(pb);
    const guint8 *pixels = gdk_pixbuf_read_pixels(pb);
    if (dw <= 0 || dh <= 0 || !pixels || gdk_pixbuf_get_n_channels(pb) != 4) {
        g_object_unref(pb);
        return 0;
    }
    if (out_src_w) *out_src_w = (uint32_t)sw;
    if (out_src_h) *out_src_h = (uint32_t)sh;
    if (out_dst_w) *out_dst_w = (uint32_t)dw;
    if (out_dst_h) *out_dst_h = (uint32_t)dh;
    size_t row_bytes = (size_t)dw * 4;
    for (int y = 0; y < dh; y++) {
        rgba_cb(ctx, pixels + (size_t)y * (size_t)stride, row_bytes);
    }
    g_object_unref(pb);
    return 1;
#else
    (void)path; (void)max_w; (void)max_h; (void)out_src_w; (void)out_src_h;
    (void)out_dst_w; (void)out_dst_h; (void)rgba_cb; (void)ctx;
    return 0;
#endif
}

#ifdef HAVE_LIBSECRET_1
static const SecretSchema TOMOYO_SECRET_SCHEMA = {
    "dev.tomoyo.secrets",
    SECRET_SCHEMA_NONE,
    {
        { "key", SECRET_SCHEMA_ATTRIBUTE_STRING },
        { NULL, 0 }
    },
    0, NULL, NULL, NULL, NULL, NULL, NULL, NULL
};
#endif

int tomoyo_secret_store(const char *key, const char *val) {
#ifdef HAVE_LIBSECRET_1
    GError *err = NULL;
    gboolean ok = secret_password_store_sync(
        &TOMOYO_SECRET_SCHEMA, SECRET_COLLECTION_DEFAULT,
        key, val, NULL, &err, "key", key, NULL
    );
    if (err) g_error_free(err);
    return ok ? 1 : 0;
#else
    (void)key; (void)val;
    return 0;
#endif
}

int tomoyo_secret_lookup(const char *key, tomoyo_str_cb out_cb, void *ctx) {
#ifdef HAVE_LIBSECRET_1
    GError *err = NULL;
    gchar *pw = secret_password_lookup_sync(&TOMOYO_SECRET_SCHEMA, NULL, &err, "key", key, NULL);
    if (err) g_error_free(err);
    if (!pw) return 0;
    if (out_cb) out_cb(ctx, pw, strlen(pw));
    secret_password_free(pw);
    return 1;
#else
    (void)key; (void)out_cb; (void)ctx;
    return 0;
#endif
}

int tomoyo_secret_clear(const char *key) {
#ifdef HAVE_LIBSECRET_1
    GError *err = NULL;
    gboolean ok = secret_password_clear_sync(&TOMOYO_SECRET_SCHEMA, NULL, &err, "key", key, NULL);
    if (err) g_error_free(err);
    return ok ? 1 : 0;
#else
    (void)key;
    return 0;
#endif
}
