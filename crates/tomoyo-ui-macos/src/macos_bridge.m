#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>
#import <stdint.h>

typedef void (*tomoyo_mac_hk_cb)(void *ctx, uint32_t hotkey_id);

typedef struct {
    EventHotKeyRef hk_ref;
    EventHandlerRef handler_ref;
    tomoyo_mac_hk_cb cb;
    void *ctx;
    uint32_t id;
} tomoyo_mac_hk_t;

static NSScreen *tomoyo_active_screen(void) {
    NSPoint mouse = [NSEvent mouseLocation];
    for (NSScreen *scr in [NSScreen screens]) {
        if (NSPointInRect(mouse, scr.frame)) {
            return scr;
        }
    }
    return [NSScreen mainScreen] ?: [[NSScreen screens] firstObject];
}

int tomoyo_macos_primary_visible_frame(int *ox, int *oy, int *ow, int *oh) {
    @autoreleasepool {
        NSScreen *scr = tomoyo_active_screen();
        if (!scr) return 0;
        NSRect vf = scr.visibleFrame;
        if (ox) *ox = (int)vf.origin.x;
        if (oy) *oy = (int)vf.origin.y;
        if (ow) *ow = (int)vf.size.width;
        if (oh) *oh = (int)vf.size.height;
        return 1;
    }
}

void *tomoyo_macos_panel_create(int x, int y, int w, int h, int corner_radius) {
    @autoreleasepool {
        NSRect rect = NSMakeRect((CGFloat)x, (CGFloat)y, (CGFloat)w, (CGFloat)h);
        NSWindowStyleMask style = NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel;
        NSPanel *panel = [[NSPanel alloc] initWithContentRect:rect
                                                    styleMask:style
                                                      backing:NSBackingStoreBuffered
                                                        defer:NO];
        panel.level = NSFloatingWindowLevel;
        panel.collectionBehavior =
            NSWindowCollectionBehaviorCanJoinAllSpaces |
            NSWindowCollectionBehaviorFullScreenAuxiliary |
            NSWindowCollectionBehaviorTransient;
        panel.hidesOnDeactivate = YES;
        panel.opaque = NO;
        panel.backgroundColor = [NSColor clearColor];
        panel.hasShadow = YES;

        NSVisualEffectView *fx = [[NSVisualEffectView alloc] initWithFrame:NSMakeRect(0, 0, (CGFloat)w, (CGFloat)h)];
        fx.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
        fx.blendingMode = NSVisualEffectBlendingModeBehindWindow;
        fx.material = NSVisualEffectMaterialHUDWindow;
        fx.state = NSVisualEffectStateActive;
        fx.wantsLayer = YES;
        if (fx.layer && corner_radius > 0) {
            fx.layer.cornerRadius = (CGFloat)corner_radius;
            fx.layer.masksToBounds = YES;
        }
        panel.contentView = fx;
        return (__bridge_retained void *)panel;
    }
}

void tomoyo_macos_panel_show(void *panel_ptr, int x, int y, int w, int h) {
    if (!panel_ptr) return;
    @autoreleasepool {
        NSPanel *panel = (__bridge NSPanel *)panel_ptr;
        [panel setFrame:NSMakeRect((CGFloat)x, (CGFloat)y, (CGFloat)w, (CGFloat)h) display:YES];
        [panel makeKeyAndOrderFront:nil];
        [NSApp activateIgnoringOtherApps:YES];
    }
}

void tomoyo_macos_panel_hide(void *panel_ptr) {
    if (!panel_ptr) return;
    @autoreleasepool {
        NSPanel *panel = (__bridge NSPanel *)panel_ptr;
        [panel orderOut:nil];
    }
}

void tomoyo_macos_panel_destroy(void *panel_ptr) {
    if (!panel_ptr) return;
    @autoreleasepool {
        NSPanel *panel = (__bridge_transfer NSPanel *)panel_ptr;
        [panel close];
    }
}

static OSStatus tomoyo_carbon_hk_handler(EventHandlerCallRef next, EventRef ev, void *ud) {
    (void)next;
    tomoyo_mac_hk_t *h = (tomoyo_mac_hk_t *)ud;
    if (!h || !h->cb) return eventNotHandledErr;
    EventHotKeyID hkid;
    OSStatus st = GetEventParameter(ev, kEventParamDirectObject, typeEventHotKeyID, NULL, sizeof(hkid), NULL, &hkid);
    if (st == noErr && hkid.id == h->id) {
        h->cb(h->ctx, hkid.id);
        return noErr;
    }
    return eventNotHandledErr;
}

void *tomoyo_macos_hotkey_register(
    uint32_t carbon_mods,
    uint16_t keycode,
    uint32_t id,
    tomoyo_mac_hk_cb cb,
    void *ctx
) {
    tomoyo_mac_hk_t *h = (tomoyo_mac_hk_t *)calloc(1, sizeof(tomoyo_mac_hk_t));
    if (!h) return NULL;
    h->cb = cb;
    h->ctx = ctx;
    h->id = id;

    EventTypeSpec spec = { kEventClassKeyboard, kEventHotKeyPressed };
    if (InstallApplicationEventHandler(NewEventHandlerUPP(tomoyo_carbon_hk_handler), 1, &spec, h, &h->handler_ref) != noErr) {
        free(h);
        return NULL;
    }
    EventHotKeyID hkid = { 'TMYO', id };
    if (RegisterEventHotKey((UInt32)keycode, (UInt32)carbon_mods, hkid, GetApplicationEventTarget(), 0, &h->hk_ref) != noErr) {
        RemoveEventHandler(h->handler_ref);
        free(h);
        return NULL;
    }
    return h;
}

void tomoyo_macos_hotkey_unregister(void *hk_ptr) {
    tomoyo_mac_hk_t *h = (tomoyo_mac_hk_t *)hk_ptr;
    if (!h) return;
    if (h->hk_ref) UnregisterEventHotKey(h->hk_ref);
    if (h->handler_ref) RemoveEventHandler(h->handler_ref);
    free(h);
}
