// SPDX-License-Identifier: GPL-3.0-or-later

//! The IMKInputController subclass. IMKServer creates one instance per
//! input session (roughly: per text input client).

use log::debug;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
use objc2_foundation::NSUInteger;
use objc2_input_method_kit::IMKInputController;

define_class!(
    // SAFETY:
    // - IMKInputController has no subclassing requirements beyond overriding
    //   the documented methods below.
    // - ChewingController does not implement Drop.
    #[unsafe(super(IMKInputController))]
    // Must match InputMethodServerControllerClass in Info.plist.
    #[name = "ChewingController"]
    pub struct ChewingController;

    impl ChewingController {
        /// Which events are delivered to `handleEvent:client:`. FlagsChanged is
        /// needed later to detect a lone Shift press (switching Chinese/English).
        #[unsafe(method(recognizedEvents:))]
        fn recognized_events(&self, _sender: Option<&AnyObject>) -> NSUInteger {
            (NSEventMask::KeyDown | NSEventMask::FlagsChanged).0 as NSUInteger
        }

        /// Return true if the event was consumed by the input method.
        #[unsafe(method(handleEvent:client:))]
        fn handle_event(&self, event: Option<&NSEvent>, _sender: Option<&AnyObject>) -> bool {
            if let Some(event) = event {
                log_event(event);
            }
            // Phase 0: observe only, let every key through to the client.
            false
        }

        #[unsafe(method(activateServer:))]
        fn activate_server(&self, sender: Option<&AnyObject>) {
            debug!("activateServer");
            // SAFETY: forwarding the same argument to the superclass.
            let _: () = unsafe { msg_send![super(self), activateServer: sender] };
        }

        #[unsafe(method(deactivateServer:))]
        fn deactivate_server(&self, sender: Option<&AnyObject>) {
            debug!("deactivateServer");
            // SAFETY: forwarding the same argument to the superclass.
            let _: () = unsafe { msg_send![super(self), deactivateServer: sender] };
        }
    }
);

fn log_event(event: &NSEvent) {
    let ty = event.r#type();
    let flags = event.modifierFlags().0;
    if ty == NSEventType::KeyDown {
        // `characters` raises an exception for non-key events, so only read
        // it for KeyDown.
        let chars = event
            .characters()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let raw = event
            .charactersIgnoringModifiers()
            .map(|s| s.to_string())
            .unwrap_or_default();
        debug!(
            "keyDown keyCode={} chars={chars:?} raw={raw:?} flags={flags:#x}",
            event.keyCode()
        );
    } else if ty == NSEventType::FlagsChanged {
        debug!("flagsChanged keyCode={} flags={flags:#x}", event.keyCode());
    } else {
        debug!("event type={}", ty.0);
    }
}
