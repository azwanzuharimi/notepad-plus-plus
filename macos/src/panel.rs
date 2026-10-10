// SPDX-License-Identifier: GPL-3.0-or-later
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSControlStateValueOff, NSControlStateValueOn, NSPanel,
    NSTextField, NSView, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub struct Form {
    pub panel: Retained<NSPanel>,
    view: Retained<NSView>,
    height: f64,
    mtm: MainThreadMarker,
}

impl Form {
    pub fn new(mtm: MainThreadMarker, title: &str, w: f64, h: f64) -> Form {
        let rect = NSRect::new(NSPoint::new(0., 0.), NSSize::new(w, h));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::UtilityWindow;
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setTitle(&NSString::from_str(title));
        panel.setFloatingPanel(true);
        panel.center();
        let view = panel.contentView().unwrap();
        Form {
            panel,
            view,
            height: h,
            mtm,
        }
    }

    pub fn place(&self, v: &NSView, x: f64, top: f64, w: f64, h: f64) {
        v.setFrame(NSRect::new(
            NSPoint::new(x, self.height - top - h),
            NSSize::new(w, h),
        ));
        self.view.addSubview(v);
    }

    pub fn label(&self, text: &str, x: f64, top: f64, w: f64) {
        let l = NSTextField::labelWithString(&NSString::from_str(text), self.mtm);
        self.place(&l, x, top + 3., w, 18.);
    }

    pub fn field(&self, x: f64, top: f64, w: f64) -> Retained<NSTextField> {
        let f = NSTextField::textFieldWithString(&NSString::new(), self.mtm);
        self.place(&f, x, top, w, 22.);
        f
    }

    pub fn status(&self, x: f64, top: f64, w: f64, h: f64) -> Retained<NSTextField> {
        let l = NSTextField::wrappingLabelWithString(&NSString::new(), self.mtm);
        self.place(&l, x, top, w, h);
        l
    }

    pub fn button(
        &self,
        title: &str,
        x: f64,
        top: f64,
        w: f64,
        target: &AnyObject,
        action: Sel,
    ) -> Retained<NSButton> {
        let b = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(title),
                Some(target),
                Some(action),
                self.mtm,
            )
        };
        self.place(&b, x, top, w, 28.);
        b
    }

    pub fn check(
        &self,
        title: &str,
        x: f64,
        top: f64,
        w: f64,
        target: &AnyObject,
        action: Sel,
    ) -> Retained<NSButton> {
        let b = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str(title),
                Some(target),
                Some(action),
                self.mtm,
            )
        };
        self.place(&b, x, top, w, 20.);
        b
    }
}

pub fn on(b: &NSButton) -> bool {
    b.state() == NSControlStateValueOn
}

pub fn set_on(b: &NSButton, v: bool) {
    b.setState(if v {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

pub fn text(f: &NSTextField) -> String {
    f.stringValue().to_string()
}
