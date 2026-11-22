#define _GNU_SOURCE
#include <pipewire/pipewire.h>
#include <spa/buffer/buffer.h>
#include <spa/param/buffers.h>
#include <spa/param/video/format-utils.h>
#include <spa/param/video/raw-utils.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>

typedef struct {
    struct pw_thread_loop *loop;
    struct pw_context *ctx;
    struct pw_core *core;
    struct pw_stream *stream;
    struct spa_hook listener;
    pthread_mutex_t mu;
    pthread_cond_t cv;
    uint8_t *staging;
    size_t staging_cap;
    int crop_x, crop_y, crop_w, crop_h;
    int src_w, src_h;
    int is_rgb0;
    int active;
    uint64_t seq;
    int failed;
    char err[256];
} pw_rec_t;

static void set_err(pw_rec_t *r, const char *msg) {
    pthread_mutex_lock(&r->mu);
    r->failed = 1;
    if (msg && !r->err[0]) snprintf(r->err, sizeof(r->err), "%s", msg);
    pthread_cond_broadcast(&r->cv);
    pthread_mutex_unlock(&r->mu);
}

static void on_state_changed(void *ud, enum pw_stream_state old, enum pw_stream_state state, const char *err) {
    (void)old;
    pw_rec_t *r = (pw_rec_t *)ud;
    if (state == PW_STREAM_STATE_ERROR) set_err(r, err ? err : "PipeWire stream error");
}

static void on_param_changed(void *ud, uint32_t id, const struct spa_pod *param) {
    pw_rec_t *r = (pw_rec_t *)ud;
    if (!param || id != SPA_PARAM_Format) return;
    struct spa_video_info_raw info;
    memset(&info, 0, sizeof(info));
    if (spa_format_video_raw_parse(param, &info) < 0) return;

    pthread_mutex_lock(&r->mu);
    r->src_w = (int)info.size.width;
    r->src_h = (int)info.size.height;
    r->is_rgb0 = (info.format == SPA_VIDEO_FORMAT_RGBA || info.format == SPA_VIDEO_FORMAT_RGBx) ? 1 : 0;
    if (r->crop_w <= 0 || r->crop_h <= 0 || r->crop_x < 0 || r->crop_y < 0 ||
        r->crop_x + r->crop_w > r->src_w || r->crop_y + r->crop_h > r->src_h) {
        r->crop_x = 0; r->crop_y = 0;
        r->crop_w = (r->src_w & ~1);
        r->crop_h = (r->src_h & ~1);
    }
    size_t need = (size_t)r->crop_w * (size_t)r->crop_h * 4;
    if (need > r->staging_cap) {
        uint8_t *nb = (uint8_t *)realloc(r->staging, need);
        if (nb) { r->staging = nb; r->staging_cap = need; }
    }
    pthread_mutex_unlock(&r->mu);

    uint8_t buf[256];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(buf, sizeof(buf));
    const struct spa_pod *p[1];
    p[0] = (const struct spa_pod *)spa_pod_builder_add_object(
        &b, SPA_TYPE_OBJECT_ParamBuffers, SPA_PARAM_Buffers,
        SPA_PARAM_BUFFERS_dataType, SPA_POD_CHOICE_FLAGS_Int((1 << SPA_DATA_MemFd) | (1 << SPA_DATA_MemPtr)));
    pw_stream_update_params(r->stream, p, 1);
}

static void on_process(void *ud) {
    pw_rec_t *r = (pw_rec_t *)ud;
    struct pw_buffer *b = NULL;
    struct pw_buffer *next;
    while ((next = pw_stream_dequeue_buffer(r->stream)) != NULL) {
        if (b) pw_stream_queue_buffer(r->stream, b);
        b = next;
    }
    if (!b) return;
    struct spa_data *d = &b->buffer->datas[0];
    if (!d->chunk || d->chunk->size == 0 || (d->chunk->flags & SPA_CHUNK_FLAG_CORRUPTED)) {
        pw_stream_queue_buffer(r->stream, b);
        return;
    }
    const uint8_t *base = (const uint8_t *)d->data;
    void *mapped = NULL;
    if (!base && d->type == SPA_DATA_MemFd && d->fd >= 0 && d->maxsize > 0) {
        mapped = mmap(NULL, d->maxsize, PROT_READ, MAP_SHARED, (int)d->fd, (off_t)d->mapoffset);
        if (mapped != MAP_FAILED) base = (const uint8_t *)mapped;
    }
    if (base && d->chunk->offset < d->maxsize) {
        const uint8_t *src = base + d->chunk->offset;
        size_t avail = d->maxsize - d->chunk->offset;
        pthread_mutex_lock(&r->mu);
        if (r->src_w > 0 && r->src_h > 0 && r->crop_w > 0 && r->crop_h > 0 && r->staging) {
            int stride = d->chunk->stride > 0 ? d->chunk->stride : r->src_w * 4;
            size_t row_bytes = (size_t)r->crop_w * 4;
            size_t need = (size_t)(r->crop_y + r->crop_h - 1) * (size_t)stride + (size_t)(r->crop_x + r->crop_w) * 4;
            if (need <= avail) {
                for (int y = 0; y < r->crop_h; y++) {
                    memcpy(r->staging + (size_t)y * row_bytes,
                           src + (size_t)(r->crop_y + y) * (size_t)stride + (size_t)r->crop_x * 4,
                           row_bytes);
                }
                r->seq++;
                pthread_cond_broadcast(&r->cv);
            }
        }
        pthread_mutex_unlock(&r->mu);
    }
    if (mapped && mapped != MAP_FAILED) munmap(mapped, d->maxsize);
    pw_stream_queue_buffer(r->stream, b);
}

static const struct pw_stream_events g_pw_ev = {
    .version = PW_VERSION_STREAM_EVENTS,
    .state_changed = on_state_changed,
    .param_changed = on_param_changed,
    .process = on_process,
};

void *tomoyo_pw_rec_start(int pw_fd, uint32_t node_id, int cx, int cy, int cw, int ch, int fps) {
    pw_init(NULL, NULL);
    pw_rec_t *r = (pw_rec_t *)calloc(1, sizeof(pw_rec_t));
    if (!r) return NULL;
    pthread_mutex_init(&r->mu, NULL);
    pthread_cond_init(&r->cv, NULL);
    r->crop_x = cx; r->crop_y = cy; r->crop_w = cw; r->crop_h = ch;
    r->active = 1;

    r->loop = pw_thread_loop_new("tomoyo-pw-rec", NULL);
    if (!r->loop) { free(r); return NULL; }
    pw_thread_loop_lock(r->loop);
    r->ctx = pw_context_new(pw_thread_loop_get_loop(r->loop), NULL, 0);
    r->core = r->ctx ? pw_context_connect_fd(r->ctx, pw_fd, NULL, 0) : NULL;
    if (!r->core) {
        pw_thread_loop_unlock(r->loop);
        pw_thread_loop_destroy(r->loop);
        if (r->ctx) pw_context_destroy(r->ctx);
        free(r);
        return NULL;
    }
    struct pw_properties *props = pw_properties_new(
        PW_KEY_MEDIA_TYPE, "Video", PW_KEY_MEDIA_CATEGORY, "Capture", PW_KEY_MEDIA_ROLE, "Screen", NULL);
    r->stream = pw_stream_new(r->core, "Tomoyo ScreenCast", props);
    pw_stream_add_listener(r->stream, &r->listener, &g_pw_ev, r);

    uint8_t pod[1024];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(pod, sizeof(pod));
    struct spa_rectangle def_sz = SPA_RECTANGLE(320, 240), min_sz = SPA_RECTANGLE(1, 1), max_sz = SPA_RECTANGLE(8192, 8192);
    struct spa_fraction def_fr = SPA_FRACTION((uint32_t)(fps > 0 ? fps : 30), 1);
    struct spa_fraction min_fr = SPA_FRACTION(0, 1), max_fr = SPA_FRACTION(240, 1);
    const struct spa_pod *params[1];
    params[0] = (const struct spa_pod *)spa_pod_builder_add_object(
        &b, SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat,
        SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video),
        SPA_FORMAT_mediaSubtype, SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
        SPA_FORMAT_VIDEO_format, SPA_POD_CHOICE_ENUM_Id(5,
            SPA_VIDEO_FORMAT_BGRA, SPA_VIDEO_FORMAT_BGRA, SPA_VIDEO_FORMAT_BGRx,
            SPA_VIDEO_FORMAT_RGBA, SPA_VIDEO_FORMAT_RGBx),
        SPA_FORMAT_VIDEO_size, SPA_POD_CHOICE_RANGE_Rectangle(&def_sz, &min_sz, &max_sz),
        SPA_FORMAT_VIDEO_framerate, SPA_POD_CHOICE_RANGE_Fraction(&def_fr, &min_fr, &max_fr));

    pw_stream_connect(r->stream, PW_DIRECTION_INPUT, node_id,
                      PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS, params, 1);
    pw_thread_loop_unlock(r->loop);
    pw_thread_loop_start(r->loop);
    return r;
}

int tomoyo_pw_rec_wait_first(void *h, int timeout_ms, int *ow, int *oh, int *is_rgb0) {
    pw_rec_t *r = (pw_rec_t *)h;
    if (!r) return 0;
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec += timeout_ms / 1000;
    ts.tv_nsec += (long)(timeout_ms % 1000) * 1000000L;
    if (ts.tv_nsec >= 1000000000L) { ts.tv_sec++; ts.tv_nsec -= 1000000000L; }

    pthread_mutex_lock(&r->mu);
    while (r->seq == 0 && !r->failed && r->active) {
        if (pthread_cond_timedwait(&r->cv, &r->mu, &ts) != 0) break;
    }
    int ok = (r->seq > 0 && !r->failed);
    if (ok) { *ow = r->crop_w; *oh = r->crop_h; *is_rgb0 = r->is_rgb0; }
    pthread_mutex_unlock(&r->mu);
    return ok;
}

int tomoyo_pw_rec_copy_latest(void *h, uint8_t *dst, size_t dst_len) {
    pw_rec_t *r = (pw_rec_t *)h;
    if (!r || !dst) return 0;
    pthread_mutex_lock(&r->mu);
    size_t need = (size_t)r->crop_w * (size_t)r->crop_h * 4;
    int ok = (r->seq > 0 && r->staging && need > 0 && need <= dst_len);
    if (ok) memcpy(dst, r->staging, need);
    pthread_mutex_unlock(&r->mu);
    return ok;
}

const char *tomoyo_pw_rec_error(void *h) {
    pw_rec_t *r = (pw_rec_t *)h;
    return (r && r->err[0]) ? r->err : NULL;
}

void tomoyo_pw_rec_stop(void *h) {
    pw_rec_t *r = (pw_rec_t *)h;
    if (!r) return;
    pthread_mutex_lock(&r->mu);
    r->active = 0;
    pthread_cond_broadcast(&r->cv);
    pthread_mutex_unlock(&r->mu);
    if (r->loop) {
        pw_thread_loop_lock(r->loop);
        if (r->stream) { pw_stream_disconnect(r->stream); pw_stream_destroy(r->stream); r->stream = NULL; }
        if (r->core) { pw_core_disconnect(r->core); r->core = NULL; }
        if (r->ctx) { pw_context_destroy(r->ctx); r->ctx = NULL; }
        pw_thread_loop_unlock(r->loop);
        pw_thread_loop_stop(r->loop);
        pw_thread_loop_destroy(r->loop);
    }
    free(r->staging);
    pthread_mutex_destroy(&r->mu);
    pthread_cond_destroy(&r->cv);
    free(r);
}
