//! macOS integration: files opened through Finder ("Open With", double-click,
//! dropping on the Dock icon).
//!
//! macOS delivers those as an `odoc` Apple Event, which AppKit turns into a call
//! to `application:openURLs:` on the NSApplication delegate. winit's delegate has
//! no such method, so AppKit shows "ExeyVue cannot open files in the Image
//! format". We add the method to winit's delegate class at runtime and hand the
//! paths to the iced app through a subscription stream.
//!
//! [`install`] must run on the main thread after winit created its delegate and
//! before the event loop starts (iced's boot function is exactly that moment), so
//! that a launch-by-double-click is caught as well.

use std::path::PathBuf;
use std::sync::Mutex;

use iced::futures::channel::mpsc;
use iced::futures::Stream;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::sel;
use objc2_app_kit::NSApplication;
use objc2_foundation::{MainThreadMarker, NSArray, NSURL};

struct Inbox {
    /// Batches that arrived before the subscription started listening.
    pending: Vec<Vec<PathBuf>>,
    sender: Option<mpsc::UnboundedSender<Vec<PathBuf>>>,
}

static INBOX: Mutex<Inbox> = Mutex::new(Inbox {
    pending: Vec::new(),
    sender: None,
});

/// Teach winit's application delegate `application:openURLs:`.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: plain accessor; some objc2-app-kit versions mark it unsafe.
    #[allow(unused_unsafe)]
    let Some(delegate) = (unsafe { app.delegate() }) else {
        return;
    };
    // SAFETY: ProtocolObject<..> is a transparent wrapper around AnyObject.
    let object: &AnyObject = unsafe { &*Retained::as_ptr(&delegate).cast::<AnyObject>() };
    let class: &AnyClass = object.class();

    let handler: extern "C" fn(&AnyObject, Sel, &NSApplication, &NSArray<NSURL>) =
        application_open_urls;
    // SAFETY: the handler's signature matches the "v@:@@" type encoding below,
    // and adding a method to an existing class is what class_addMethod is for.
    unsafe {
        let imp: objc2::ffi::IMP = std::mem::transmute(handler);
        objc2::ffi::class_addMethod(
            (class as *const AnyClass as *mut AnyClass).cast(),
            sel!(application:openURLs:).as_ptr().cast(),
            imp,
            c"v@:@@".as_ptr(),
        );
    }
}

extern "C" fn application_open_urls(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &NSApplication,
    urls: &NSArray<NSURL>,
) {
    let mut paths = Vec::new();
    for i in 0..urls.count() {
        // SAFETY: `i` is in bounds (0..urls.count()), and reading a URL's path is a
        // plain accessor; some objc2 versions mark both unsafe.
        unsafe {
            let url = urls.objectAtIndex(i);
            if let Some(path) = url.path() {
                paths.push(PathBuf::from(path.to_string()));
            }
        }
    }
    if !paths.is_empty() {
        deliver(paths);
    }
}

fn deliver(paths: Vec<PathBuf>) {
    let mut inbox = INBOX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(sender) = &inbox.sender {
        if sender.unbounded_send(paths.clone()).is_ok() {
            return;
        }
        inbox.sender = None;
    }
    inbox.pending.push(paths);
}

/// Stream of "open these files" requests, for `Subscription::run`.
pub fn opened_files() -> impl Stream<Item = Vec<PathBuf>> + Send {
    let (sender, receiver) = mpsc::unbounded();
    let mut inbox = INBOX.lock().unwrap_or_else(|e| e.into_inner());
    for paths in inbox.pending.drain(..) {
        let _ = sender.unbounded_send(paths);
    }
    inbox.sender = Some(sender);
    receiver
}
