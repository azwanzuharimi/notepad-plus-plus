// SPDX-License-Identifier: GPL-3.0-or-later
use crate::search::{Mode, Opts};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{sel, MainThreadMarker, MainThreadOnly};
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

    fn place(&self, v: &NSView, x: f64, top: f64, w: f64, h: f64) {
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

    fn radio(
        &self,
        title: &str,
        x: f64,
        top: f64,
        w: f64,
        target: &AnyObject,
    ) -> Retained<NSButton> {
        let b = unsafe {
            NSButton::radioButtonWithTitle_target_action(
                &NSString::from_str(title),
                Some(target),
                Some(sel!(searchModeChanged:)),
                self.mtm,
            )
        };
        self.place(&b, x, top, w, 20.);
        b
    }
}

pub struct Controls {
    pub find: Retained<NSTextField>,
    pub replace: Retained<NSTextField>,
    pub whole: Retained<NSButton>,
    pub case: Retained<NSButton>,
    pub wrap: Option<Retained<NSButton>>,
    pub modes: [Retained<NSButton>; 3],
    pub dot_nl: Retained<NSButton>,
    pub status: Retained<NSTextField>,
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

// Find what, Replace with, then `extra` rows of other fields, then options and Search Mode.
pub fn controls(
    f: &Form,
    t: &AnyObject,
    extra: f64,
    checks: &[&str],
) -> (Controls, Vec<Retained<NSButton>>) {
    let noop = sel!(searchModeChanged:);
    f.label("Find what:", 16., 16., 100.);
    let find = f.field(120., 16., 300.);
    f.label("Replace with:", 16., 46., 100.);
    let replace = f.field(120., 46., 300.);
    let mut top = 84. + extra;
    let mut boxes = vec![];
    for c in checks {
        boxes.push(f.check(c, 16., top, 260., t, noop));
        top += 24.;
    }
    let whole = f.check("Match whole word only", 16., top, 260., t, noop);
    let case = f.check("Match case", 16., top + 24., 260., t, noop);
    let wrap = (extra == 0.).then(|| f.check("Wrap around", 16., top + 48., 260., t, noop));
    top += if extra == 0. { 80. } else { 56. };
    f.label("Search Mode", 16., top, 200.);
    let modes = [
        f.radio("Normal", 24., top + 22., 300., t),
        f.radio(
            "Extended (\\n, \\r, \\t, \\0, \\x...)",
            24.,
            top + 44.,
            300.,
            t,
        ),
        f.radio("Regular expression", 24., top + 66., 170., t),
    ];
    let dot_nl = f.check(". matches newline", 200., top + 66., 160., t, noop);
    set_on(&modes[0], true);
    dot_nl.setEnabled(false);
    if let Some(w) = &wrap {
        set_on(w, true);
    }
    let status = f.status(16., top + 96., 520., 36.);
    (
        Controls {
            find,
            replace,
            whole,
            case,
            wrap,
            modes,
            dot_nl,
            status,
        },
        boxes,
    )
}

impl Controls {
    pub fn opts(&self) -> Opts {
        let mode = match self.modes.iter().position(|b| on(b)) {
            Some(1) => Mode::Extended,
            Some(2) => Mode::Regex,
            _ => Mode::Normal,
        };
        Opts {
            find: text(&self.find),
            replace: text(&self.replace),
            whole_word: on(&self.whole),
            match_case: on(&self.case),
            wrap: self.wrap.as_ref().is_none_or(|w| on(w)),
            mode,
            dot_nl: on(&self.dot_nl),
        }
    }

    pub fn set_status(&self, s: &str) {
        self.status.setStringValue(&NSString::from_str(s));
    }

    pub fn mode_changed(&self) {
        self.dot_nl.setEnabled(on(&self.modes[2]));
    }
}
