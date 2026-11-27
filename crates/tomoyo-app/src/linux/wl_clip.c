#include <wayland-client.h>
#include <errno.h>
#include <poll.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef void (*wl_clip_notify_cb)(void);

static const struct wl_interface ext_mgr_iface, ext_dev_iface, ext_src_iface, ext_off_iface;
static const struct wl_interface wlr_mgr_iface, wlr_dev_iface, wlr_src_iface, wlr_off_iface;

static const struct wl_interface *ext_mgr_create[] = { &ext_src_iface };
static const struct wl_interface *ext_mgr_dev[] = { &ext_dev_iface, &wl_seat_interface };
static const struct wl_message ext_mgr_reqs[] = {
    { "create_data_source", "n", ext_mgr_create },
    { "get_data_device", "no", ext_mgr_dev },
    { "destroy", "", NULL },
};
static const struct wl_interface ext_mgr_iface = { "ext_data_control_manager_v1", 1, 3, ext_mgr_reqs, 0, NULL };

static const struct wl_interface *ext_dev_sel[] = { &ext_src_iface };
static const struct wl_interface *ext_dev_off[] = { &ext_off_iface };
static const struct wl_message ext_dev_reqs[] = {
    { "set_selection", "?o", ext_dev_sel },
    { "destroy", "", NULL },
    { "set_primary_selection", "?o", ext_dev_sel },
};
static const struct wl_message ext_dev_evts[] = {
    { "data_offer", "n", ext_dev_off },
    { "selection", "?o", ext_dev_off },
    { "finished", "", NULL },
    { "primary_selection", "?o", ext_dev_off },
};
static const struct wl_interface ext_dev_iface = { "ext_data_control_device_v1", 1, 3, ext_dev_reqs, 4, ext_dev_evts };

static const struct wl_interface *null_types[] = { NULL, NULL };
static const struct wl_message src_reqs[] = { { "offer", "s", null_types }, { "destroy", "", NULL } };
static const struct wl_message src_evts[] = { { "send", "sh", null_types }, { "cancelled", "", NULL } };
static const struct wl_interface ext_src_iface = { "ext_data_control_source_v1", 1, 2, src_reqs, 2, src_evts };

static const struct wl_message off_reqs[] = { { "receive", "sh", null_types }, { "destroy", "", NULL } };
static const struct wl_message off_evts[] = { { "offer", "s", null_types } };
static const struct wl_interface ext_off_iface = { "ext_data_control_offer_v1", 1, 2, off_reqs, 1, off_evts };

static const struct wl_interface *wlr_mgr_create[] = { &wlr_src_iface };
static const struct wl_interface *wlr_mgr_dev[] = { &wlr_dev_iface, &wl_seat_interface };
static const struct wl_message wlr_mgr_reqs[] = {
    { "create_data_source", "n", wlr_mgr_create },
    { "get_data_device", "no", wlr_mgr_dev },
    { "destroy", "", NULL },
};
static const struct wl_interface wlr_mgr_iface = { "zwlr_data_control_manager_v1", 2, 3, wlr_mgr_reqs, 0, NULL };

static const struct wl_interface *wlr_dev_sel[] = { &wlr_src_iface };
static const struct wl_interface *wlr_dev_off[] = { &wlr_off_iface };
static const struct wl_message wlr_dev_reqs[] = {
    { "set_selection", "?o", wlr_dev_sel },
    { "destroy", "", NULL },
    { "set_primary_selection", "?o", wlr_dev_sel },
};
static const struct wl_message wlr_dev_evts[] = {
    { "data_offer", "n", wlr_dev_off },
    { "selection", "?o", wlr_dev_off },
    { "finished", "", NULL },
    { "primary_selection", "?o", wlr_dev_off },
};
static const struct wl_interface wlr_dev_iface = { "zwlr_data_control_device_v1", 2, 3, wlr_dev_reqs, 4, wlr_dev_evts };
static const struct wl_interface wlr_src_iface = { "zwlr_data_control_source_v1", 1, 2, src_reqs, 2, src_evts };
static const struct wl_interface wlr_off_iface = { "zwlr_data_control_offer_v1", 1, 2, off_reqs, 1, off_evts };

typedef struct {
    pthread_mutex_t mu;
    pthread_cond_t cv;
    struct wl_display *dpy;
    struct wl_registry *reg;
    struct wl_proxy *mgr;
    struct wl_proxy *seat;
    struct wl_proxy *dev;
    int is_ext;
    int ready;
    int stop;
    int pipe_wr;
    int pipe_rd;
    pthread_t thr;
    int started;
    wl_clip_notify_cb on_change;

    struct wl_proxy *pending_off;
    char pending_mimes[16][96];
    int pending_cnt;

    struct wl_proxy *cur_off;
    char cur_mimes[16][96];
    int cur_cnt;

    struct wl_proxy *prim_off;
    char prim_mimes[8][96];
    int prim_cnt;

    struct wl_proxy *our_src;
    uint8_t *our_data;
    size_t our_len;
    int suppress_next;

    int cmd_kind; /* 1=set, 2=read */
    char cmd_m1[96];
    char cmd_m2[96];
    const uint8_t *cmd_buf;
    size_t cmd_len;
    int cmd_fd;
    int cmd_done;
} wl_clip_t;

static wl_clip_t g_wl = {
    .mu = PTHREAD_MUTEX_INITIALIZER,
    .cv = PTHREAD_COND_INITIALIZER,
    .pipe_wr = -1,
    .pipe_rd = -1,
};

static void wake_wl(void) {
    char b = 1;
    if (g_wl.pipe_wr >= 0) { ssize_t r = write(g_wl.pipe_wr, &b, 1); (void)r; }
}

static void on_off_offer(void *ud, void *off, const char *mime) {
    (void)ud;
    if (off != g_wl.pending_off || !mime || g_wl.pending_cnt >= 16) return;
    snprintf(g_wl.pending_mimes[g_wl.pending_cnt++], 96, "%s", mime);
}
static void (*off_cbs[])(void) = { (void (*)(void))on_off_offer };

static void on_src_send(void *ud, void *src, const char *mime, int fd) {
    (void)ud; (void)mime;
    pthread_mutex_lock(&g_wl.mu);
    uint8_t *copy = NULL;
    size_t len = 0;
    if (src == g_wl.our_src && g_wl.our_data && g_wl.our_len > 0) {
        copy = (uint8_t *)malloc(g_wl.our_len);
        if (copy) { memcpy(copy, g_wl.our_data, g_wl.our_len); len = g_wl.our_len; }
    }
    pthread_mutex_unlock(&g_wl.mu);
    if (copy) {
        size_t pos = 0;
        while (pos < len) {
            ssize_t n = write(fd, copy + pos, len - pos);
            if (n < 0 && errno == EINTR) continue;
            if (n <= 0) break;
            pos += (size_t)n;
        }
        free(copy);
    }
    close(fd);
}
static void on_src_cancel(void *ud, void *src) {
    (void)ud;
    pthread_mutex_lock(&g_wl.mu);
    if (src == g_wl.our_src) {
        wl_proxy_destroy((struct wl_proxy *)src);
        g_wl.our_src = NULL;
        free(g_wl.our_data);
        g_wl.our_data = NULL;
        g_wl.our_len = 0;
    } else {
        wl_proxy_destroy((struct wl_proxy *)src);
    }
    pthread_mutex_unlock(&g_wl.mu);
}
static void (*src_cbs[])(void) = { (void (*)(void))on_src_send, (void (*)(void))on_src_cancel };

static void on_dev_data_offer(void *ud, void *dev, void *id) {
    (void)ud; (void)dev;
    if (g_wl.pending_off) wl_proxy_destroy(g_wl.pending_off);
    g_wl.pending_off = (struct wl_proxy *)id;
    g_wl.pending_cnt = 0;
    if (g_wl.pending_off) wl_proxy_add_listener(g_wl.pending_off, off_cbs, NULL);
}
static void on_dev_selection(void *ud, void *dev, void *id) {
    (void)ud; (void)dev;
    pthread_mutex_lock(&g_wl.mu);
    if (g_wl.cur_off) { wl_proxy_destroy(g_wl.cur_off); g_wl.cur_off = NULL; }
    g_wl.cur_cnt = 0;
    if (id && id == g_wl.pending_off) {
        g_wl.cur_off = g_wl.pending_off;
        g_wl.cur_cnt = g_wl.pending_cnt;
        memcpy(g_wl.cur_mimes, g_wl.pending_mimes, sizeof(g_wl.cur_mimes));
        g_wl.pending_off = NULL;
        g_wl.pending_cnt = 0;
    }
    int sup = g_wl.suppress_next;
    g_wl.suppress_next = 0;
    int has_sel = (g_wl.cur_off != NULL);
    wl_clip_notify_cb cb = g_wl.on_change;
    pthread_mutex_unlock(&g_wl.mu);
    if (!sup && has_sel && cb) cb();
}
static void on_dev_finished(void *ud, void *dev) { (void)ud; (void)dev; g_wl.ready = 0; }
static void on_dev_primary(void *ud, void *dev, void *id) {
    (void)ud; (void)dev;
    pthread_mutex_lock(&g_wl.mu);
    if (g_wl.prim_off) { wl_proxy_destroy(g_wl.prim_off); g_wl.prim_off = NULL; }
    g_wl.prim_cnt = 0;
    if (id && id == g_wl.pending_off) {
        g_wl.prim_off = g_wl.pending_off;
        g_wl.prim_cnt = g_wl.pending_cnt < 8 ? g_wl.pending_cnt : 8;
        memcpy(g_wl.prim_mimes, g_wl.pending_mimes, (size_t)g_wl.prim_cnt * 96);
        g_wl.pending_off = NULL;
        g_wl.pending_cnt = 0;
    }
    pthread_mutex_unlock(&g_wl.mu);
}
static void (*dev_cbs[])(void) = {
    (void (*)(void))on_dev_data_offer, (void (*)(void))on_dev_selection,
    (void (*)(void))on_dev_finished, (void (*)(void))on_dev_primary,
};

static void on_reg_global(void *ud, struct wl_registry *reg, uint32_t name, const char *iface, uint32_t ver) {
    (void)ud;
    if (strcmp(iface, "ext_data_control_manager_v1") == 0) {
        if (g_wl.mgr) wl_proxy_destroy(g_wl.mgr);
        g_wl.mgr = (struct wl_proxy *)wl_registry_bind(reg, name, &ext_mgr_iface, 1);
        g_wl.is_ext = 1;
    } else if (strcmp(iface, "zwlr_data_control_manager_v1") == 0 && !g_wl.mgr) {
        g_wl.mgr = (struct wl_proxy *)wl_registry_bind(reg, name, &wlr_mgr_iface, ver < 2 ? ver : 2);
        g_wl.is_ext = 0;
    } else if (strcmp(iface, "wl_seat") == 0 && !g_wl.seat) {
        g_wl.seat = (struct wl_proxy *)wl_registry_bind(reg, name, &wl_seat_interface, 1);
    }
}
static void on_reg_remove(void *ud, struct wl_registry *r, uint32_t n) { (void)ud; (void)r; (void)n; }
static const struct wl_registry_listener reg_cbs = { on_reg_global, on_reg_remove };

static void process_pending_cmd(void) {
    pthread_mutex_lock(&g_wl.mu);
    int kind = g_wl.cmd_kind;
    if (kind == 1 && g_wl.mgr && g_wl.dev) {
        if (g_wl.our_src) { wl_proxy_destroy(g_wl.our_src); g_wl.our_src = NULL; }
        free(g_wl.our_data);
        g_wl.our_data = (uint8_t *)malloc(g_wl.cmd_len > 0 ? g_wl.cmd_len : 1);
        if (g_wl.our_data && g_wl.cmd_len > 0) memcpy(g_wl.our_data, g_wl.cmd_buf, g_wl.cmd_len);
        g_wl.our_len = g_wl.cmd_len;
        const struct wl_interface *si = g_wl.is_ext ? &ext_src_iface : &wlr_src_iface;
        g_wl.our_src = wl_proxy_marshal_constructor(g_wl.mgr, 0, si, NULL);
        if (g_wl.our_src) {
            wl_proxy_add_listener(g_wl.our_src, src_cbs, NULL);
            if (g_wl.cmd_m1[0]) wl_proxy_marshal(g_wl.our_src, 0, g_wl.cmd_m1);
            if (g_wl.cmd_m2[0]) wl_proxy_marshal(g_wl.our_src, 0, g_wl.cmd_m2);
            g_wl.cur_cnt = 0;
            if (g_wl.cmd_m1[0]) snprintf(g_wl.cur_mimes[g_wl.cur_cnt++], 96, "%s", g_wl.cmd_m1);
            if (g_wl.cmd_m2[0]) snprintf(g_wl.cur_mimes[g_wl.cur_cnt++], 96, "%s", g_wl.cmd_m2);
            g_wl.suppress_next = 1;
            wl_proxy_marshal(g_wl.dev, 0, g_wl.our_src);
            wl_display_flush(g_wl.dpy);
        }
    } else if (kind == 2 || kind == 3) {
        g_wl.cmd_fd = -1;
        struct wl_proxy *off = (kind == 3) ? g_wl.prim_off : g_wl.cur_off;
        if (off) {
            int fds[2];
            if (pipe(fds) == 0) {
                wl_proxy_marshal(off, 0, g_wl.cmd_m1, fds[1]);
                wl_display_flush(g_wl.dpy);
                close(fds[1]);
                g_wl.cmd_fd = fds[0];
            }
        }
    }
    if (kind != 0) {
        g_wl.cmd_kind = 0;
        g_wl.cmd_done = 1;
        pthread_cond_broadcast(&g_wl.cv);
    }
    pthread_mutex_unlock(&g_wl.mu);
}

static void *wl_thread_main(void *arg) {
    (void)arg;
    struct pollfd pfds[2] = {
        { .fd = wl_display_get_fd(g_wl.dpy), .events = POLLIN },
        { .fd = g_wl.pipe_rd, .events = POLLIN },
    };
    while (!g_wl.stop) {
        while (wl_display_prepare_read(g_wl.dpy) != 0) {
            wl_display_dispatch_pending(g_wl.dpy);
            process_pending_cmd();
            if (g_wl.stop) { wl_display_cancel_read(g_wl.dpy); return NULL; }
        }
        wl_display_flush(g_wl.dpy);
        int rc = poll(pfds, 2, -1);
        if (rc > 0 && (pfds[0].revents & POLLIN)) wl_display_read_events(g_wl.dpy);
        else wl_display_cancel_read(g_wl.dpy);
        if (rc > 0 && (pfds[1].revents & POLLIN)) {
            char tmp[32]; ssize_t r = read(g_wl.pipe_rd, tmp, sizeof(tmp)); (void)r;
        }
        if (wl_display_dispatch_pending(g_wl.dpy) < 0) break;
        process_pending_cmd();
    }
    return NULL;
}

void tomoyo_wl_clip_start(wl_clip_notify_cb on_change) {
    if (g_wl.started) return;
    if (!getenv("WAYLAND_DISPLAY")) return;
    struct wl_display *dpy = wl_display_connect(NULL);
    if (!dpy) return;
    int pfds[2];
    if (pipe(pfds) != 0) { wl_display_disconnect(dpy); return; }
    g_wl.dpy = dpy; g_wl.pipe_rd = pfds[0]; g_wl.pipe_wr = pfds[1];
    g_wl.on_change = on_change; g_wl.stop = 0;
    g_wl.reg = wl_display_get_registry(dpy);
    wl_registry_add_listener(g_wl.reg, &reg_cbs, NULL);
    wl_display_roundtrip(dpy);
    if (g_wl.mgr && g_wl.seat) {
        const struct wl_interface *di = g_wl.is_ext ? &ext_dev_iface : &wlr_dev_iface;
        g_wl.dev = wl_proxy_marshal_constructor(g_wl.mgr, 1, di, NULL, g_wl.seat);
        if (g_wl.dev) {
            wl_proxy_add_listener(g_wl.dev, dev_cbs, NULL);
            wl_display_roundtrip(dpy);
            g_wl.ready = 1;
            g_wl.started = 1;
            pthread_create(&g_wl.thr, NULL, wl_thread_main, NULL);
            return;
        }
    }
    close(g_wl.pipe_rd); close(g_wl.pipe_wr); g_wl.pipe_rd = g_wl.pipe_wr = -1;
    wl_display_disconnect(dpy); g_wl.dpy = NULL;
}

void tomoyo_wl_clip_stop(void) {
    if (!g_wl.started) return;
    g_wl.stop = 1; g_wl.ready = 0;
    wake_wl();
    pthread_join(g_wl.thr, NULL);
    g_wl.started = 0;
    if (g_wl.our_src) { wl_proxy_destroy(g_wl.our_src); g_wl.our_src = NULL; }
    if (g_wl.cur_off) { wl_proxy_destroy(g_wl.cur_off); g_wl.cur_off = NULL; }
    if (g_wl.prim_off) { wl_proxy_destroy(g_wl.prim_off); g_wl.prim_off = NULL; }
    if (g_wl.pending_off) { wl_proxy_destroy(g_wl.pending_off); g_wl.pending_off = NULL; }
    if (g_wl.dev) { wl_proxy_marshal(g_wl.dev, 1); wl_proxy_destroy(g_wl.dev); g_wl.dev = NULL; }
    if (g_wl.mgr) { wl_proxy_marshal(g_wl.mgr, 2); wl_proxy_destroy(g_wl.mgr); g_wl.mgr = NULL; }
    if (g_wl.seat) { wl_proxy_destroy(g_wl.seat); g_wl.seat = NULL; }
    if (g_wl.reg) { wl_registry_destroy(g_wl.reg); g_wl.reg = NULL; }
    free(g_wl.our_data); g_wl.our_data = NULL; g_wl.our_len = 0;
    if (g_wl.pipe_rd >= 0) close(g_wl.pipe_rd);
    if (g_wl.pipe_wr >= 0) close(g_wl.pipe_wr);
    g_wl.pipe_rd = g_wl.pipe_wr = -1;
    if (g_wl.dpy) { wl_display_disconnect(g_wl.dpy); g_wl.dpy = NULL; }
}

int tomoyo_wl_clip_active(void) { return g_wl.ready; }
int tomoyo_wl_clip_has_mime(const char *mime) {
    if (!g_wl.ready || !mime) return 0;
    pthread_mutex_lock(&g_wl.mu);
    int found = 0;
    for (int i = 0; i < g_wl.cur_cnt; i++) {
        if (strcmp(g_wl.cur_mimes[i], mime) == 0) { found = 1; break; }
    }
    pthread_mutex_unlock(&g_wl.mu);
    return found;
}
int tomoyo_wl_clip_set(const char *m1, const char *m2, const uint8_t *data, size_t len) {
    if (!g_wl.ready || !m1 || !data) return 0;
    pthread_mutex_lock(&g_wl.mu);
    snprintf(g_wl.cmd_m1, 96, "%s", m1);
    snprintf(g_wl.cmd_m2, 96, "%s", m2 ? m2 : "");
    g_wl.cmd_buf = data; g_wl.cmd_len = len; g_wl.cmd_done = 0; g_wl.cmd_kind = 1;
    wake_wl();
    while (!g_wl.cmd_done && g_wl.ready) pthread_cond_wait(&g_wl.cv, &g_wl.mu);
    pthread_mutex_unlock(&g_wl.mu);
    return 1;
}

static uint8_t *read_fd_bounded(int fd, size_t max_bytes, size_t *out_len) {
    if (fd < 0) return NULL;
    size_t cap = 4096, len = 0;
    uint8_t *buf = (uint8_t *)malloc(cap + 1);
    if (!buf) { close(fd); return NULL; }
    struct pollfd pfd = { .fd = fd, .events = POLLIN };
    while (len < max_bytes && poll(&pfd, 1, 250) > 0) {
        if (len + 4096 > cap) {
            cap *= 2;
            uint8_t *nb = (uint8_t *)realloc(buf, cap + 1);
            if (!nb) break;
            buf = nb;
        }
        ssize_t n = read(fd, buf + len, cap - len);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) break;
        len += (size_t)n;
    }
    close(fd); buf[len] = '\0';
    if (out_len) *out_len = len;
    return buf;
}

static uint8_t *wl_issue_read(int kind, const char *mime, size_t max_bytes, size_t *out_len) {
    snprintf(g_wl.cmd_m1, 96, "%s", mime);
    g_wl.cmd_fd = -1; g_wl.cmd_done = 0; g_wl.cmd_kind = kind;
    wake_wl();
    while (!g_wl.cmd_done && g_wl.ready) pthread_cond_wait(&g_wl.cv, &g_wl.mu);
    int fd = g_wl.cmd_fd;
    pthread_mutex_unlock(&g_wl.mu);
    return read_fd_bounded(fd, max_bytes, out_len);
}

uint8_t *tomoyo_wl_clip_read(const char *mime, size_t *out_len) {
    if (out_len) *out_len = 0;
    if (!g_wl.ready || !mime) return NULL;
    pthread_mutex_lock(&g_wl.mu);
    if (g_wl.our_src && g_wl.our_data && g_wl.our_len > 0) {
        int match = 0;
        for (int i = 0; i < g_wl.cur_cnt; i++) {
            if (strcmp(g_wl.cur_mimes[i], mime) == 0) { match = 1; break; }
        }
        if (match) {
            uint8_t *copy = (uint8_t *)malloc(g_wl.our_len + 1);
            if (copy) { memcpy(copy, g_wl.our_data, g_wl.our_len); copy[g_wl.our_len] = '\0'; if (out_len) *out_len = g_wl.our_len; }
            pthread_mutex_unlock(&g_wl.mu);
            return copy;
        }
    }
    return wl_issue_read(2, mime, 32u * 1024u * 1024u, out_len);
}

uint8_t *tomoyo_wl_clip_read_primary(size_t *out_len) {
    if (out_len) *out_len = 0;
    if (!g_wl.ready) return NULL;
    static const char *mimes[] = { "text/plain;charset=utf-8", "UTF8_STRING", "text/plain" };
    pthread_mutex_lock(&g_wl.mu);
    const char *picked = NULL;
    for (int m = 0; m < 3 && !picked; m++) {
        for (int i = 0; i < g_wl.prim_cnt; i++) {
            if (strcmp(g_wl.prim_mimes[i], mimes[m]) == 0) { picked = mimes[m]; break; }
        }
    }
    if (!picked || !g_wl.prim_off) { pthread_mutex_unlock(&g_wl.mu); return NULL; }
    return wl_issue_read(3, picked, 256u * 1024u, out_len);
}
