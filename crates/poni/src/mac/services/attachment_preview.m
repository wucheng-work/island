#import <AppKit/AppKit.h>
#import <QuickLookUI/QuickLookUI.h>

@interface PoniAttachmentWindow : NSWindow
@end

@implementation PoniAttachmentWindow
- (void)cancelOperation:(id)sender {
    [self close];
}
@end

@interface PoniAttachmentPreview : NSObject <NSWindowDelegate>
@property(nonatomic, strong) NSWindow *window;
@property(nonatomic, strong) QLPreviewView *preview;
@end

@implementation PoniAttachmentPreview
- (void)windowWillClose:(NSNotification *)notification {
    // 显式关闭一次，释放系统预览资源；下次打开时重建。
    [self.preview close];
    self.preview = nil;
    self.window = nil;
}
@end

static PoniAttachmentPreview *attachmentPreview;

int poni_preview_attachment(const char *path) {
    if (![NSThread isMainThread]) return 2;
    NSString *file = [[NSString alloc] initWithUTF8String:path];
    if (!file || ![[NSFileManager defaultManager] isReadableFileAtPath:file]) return 1;

    if (!attachmentPreview) attachmentPreview = [PoniAttachmentPreview new];
    if (!attachmentPreview.window) {
        NSWindow *window = [[PoniAttachmentWindow alloc]
            initWithContentRect:NSMakeRect(0, 0, 760, 560)
            styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable
            backing:NSBackingStoreBuffered defer:NO];
        window.releasedWhenClosed = NO;
        window.minSize = NSMakeSize(360, 280);
        window.delegate = attachmentPreview;
        QLPreviewView *preview = [[QLPreviewView alloc]
            initWithFrame:window.contentView.bounds style:QLPreviewViewStyleNormal];
        if (!window || !preview) return 2;
        preview.shouldCloseWithWindow = NO;
        preview.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
        window.contentView = preview;
        attachmentPreview.preview = preview;
        attachmentPreview.window = window;
        [window center];
    }

    NSURL *url = [NSURL fileURLWithPath:file];
    attachmentPreview.preview.previewItem = url;
    attachmentPreview.window.title = [@"附件预览 — " stringByAppendingString:file.lastPathComponent];
    attachmentPreview.window.representedURL = url;
    [NSApp activateIgnoringOtherApps:YES];
    [attachmentPreview.window deminiaturize:nil];
    [attachmentPreview.window makeKeyAndOrderFront:nil];
    return 0;
}

void poni_close_attachment_preview(void) {
    if ([NSThread isMainThread]) [attachmentPreview.window close];
}
