// SPDX-License-Identifier: GPL-3.0-or-later

//! The IMKInputController subclass. IMKServer creates one instance per
//! input session (roughly: per text input client). All instances share one
//! [`ChewingCore`](chewing_imk_core::ChewingCore), see [`crate::state`].

use std::time::Duration;

use chewing_imk_core::Action;
use chewing_imk_core::keymap::{KeyInput, ModifierInput, Modifiers};
use log::{debug, warn};
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};
use objc2_foundation::NSUInteger;
use objc2_input_method_kit::IMKInputController;

use crate::{candidate_window, client, notice_window, state};

/// Keyboard layout requested from the client while we are active, so
/// physical keys produce QWERTY characters whatever layout the user picked
/// last. Zhuyin layouts themselves are handled by libchewing.
const BASE_KEYBOARD_LAYOUT: &str = "com.apple.keylayout.ABC";

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
        /// Which events are delivered to `handleEvent:client:`. FlagsChanged
        /// detects a lone Shift tap (switching Chinese/English) and Caps Lock.
        #[unsafe(method(recognizedEvents:))]
        fn recognized_events(&self, _sender: Option<&AnyObject>) -> NSUInteger {
            (NSEventMask::KeyDown | NSEventMask::FlagsChanged).0 as NSUInteger
        }

        /// Return true if the event was consumed by the input method.
        #[unsafe(method(handleEvent:client:))]
        fn handle_event(&self, event: Option<&NSEvent>, sender: Option<&AnyObject>) -> bool {
            self.handle_event_impl(event, sender)
        }

        #[unsafe(method(activateServer:))]
        fn activate_server(&self, sender: Option<&AnyObject>) {
            debug!("activateServer");
            if let Some(client) = sender {
                client::override_keyboard(client, BASE_KEYBOARD_LAYOUT);
                // Caps Lock may have changed while another input source was
                // active.
                let caps_lock =
                    NSEvent::modifierFlags_class().contains(NSEventModifierFlags::CapsLock);
                if let Some(actions) = state::with_core(|core| core.sync_caps_lock(caps_lock)) {
                    self.apply(client, actions);
                }
            }
            // SAFETY: forwarding the same argument to the superclass.
            let _: () = unsafe { msg_send![super(self), activateServer: sender] };
        }

        #[unsafe(method(deactivateServer:))]
        fn deactivate_server(&self, sender: Option<&AnyObject>) {
            debug!("deactivateServer");
            self.commit_pending(sender);
            candidate_window::hide();
            // SAFETY: forwarding the same argument to the superclass.
            let _: () = unsafe { msg_send![super(self), deactivateServer: sender] };
        }

        /// The client wants the composition finished now (e.g. the user
        /// clicked elsewhere in the document).
        #[unsafe(method(commitComposition:))]
        fn commit_composition(&self, sender: Option<&AnyObject>) {
            debug!("commitComposition");
            self.commit_pending(sender);
        }
    }
);

impl ChewingController {
    fn handle_event_impl(&self, event: Option<&NSEvent>, sender: Option<&AnyObject>) -> bool {
        let (Some(event), Some(client)) = (event, sender) else {
            return false;
        };
        let ty = event.r#type();
        let resp = if ty == NSEventType::KeyDown {
            let input = key_input(event);
            state::with_core(|core| core.handle_key(&input))
        } else if ty == NSEventType::FlagsChanged {
            let input = modifier_input(event);
            state::with_core(|core| core.handle_modifiers(&input))
        } else {
            None
        };
        let Some(resp) = resp else {
            return false;
        };
        debug!(
            "event type={} keyCode={:#x} handled={} actions={}",
            ty.0,
            event.keyCode(),
            resp.handled,
            resp.actions.len()
        );
        self.apply(client, resp.actions);
        resp.handled
    }

    fn commit_pending(&self, sender: Option<&AnyObject>) {
        let Some(actions) = state::with_core(|core| core.commit_composition()) else {
            return;
        };
        match sender {
            Some(client) => self.apply(client, actions),
            None => {
                if !actions.is_empty() {
                    warn!("dropping {} action(s): no client", actions.len());
                }
                candidate_window::hide();
            }
        }
    }

    /// Carry out the core's actions on `client`, in order. The candidate and
    /// notice windows are placed last so they can use the caret of the newest
    /// marked text.
    fn apply(&self, client: &AnyObject, actions: Vec<Action>) {
        // Index (UTF-16) of the character the candidates are for.
        let mut anchor = 0;
        let mut candidates = None;
        let mut notice = None;
        for action in actions {
            match action {
                Action::Commit(text) => client::insert_text(client, &text),
                Action::UpdateComposition(composition) => {
                    // While selecting, libchewing puts the cursor in front of
                    // the character being chosen. At the very end there is no
                    // character after the cursor, so use the last one.
                    let len = composition.text.chars().count();
                    let index = composition.cursor.min(len.saturating_sub(1));
                    anchor = client::utf16_offset(&composition.text, index);
                    client::set_marked_text(self, client, &composition);
                }
                Action::ShowCandidates(list) => candidates = Some(list),
                Action::HideCandidates => {
                    candidates = None;
                    candidate_window::hide();
                }
                Action::ShowNotice(message) => notice = Some(message),
            }
        }
        if candidates.is_none() && notice.is_none() {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            warn!("window requested off the main thread");
            return;
        };
        let line = client::line_rect(client, anchor);
        if let Some(list) = candidates {
            candidate_window::show(mtm, &list, line);
        }
        if let Some(message) = notice {
            notice_window::show(mtm, &message, line);
        }
    }
}

fn modifiers(flags: NSEventModifierFlags) -> Modifiers {
    Modifiers {
        shift: flags.contains(NSEventModifierFlags::Shift),
        control: flags.contains(NSEventModifierFlags::Control),
        option: flags.contains(NSEventModifierFlags::Option),
        command: flags.contains(NSEventModifierFlags::Command),
        caps_lock: flags.contains(NSEventModifierFlags::CapsLock),
    }
}

fn modifier_input(event: &NSEvent) -> ModifierInput {
    ModifierInput {
        key_code: event.keyCode(),
        modifiers: modifiers(event.modifierFlags()),
        // Seconds since system startup; negative or NaN would be a bug in
        // AppKit, clamp rather than panic.
        timestamp: Duration::from_secs_f64(event.timestamp().max(0.0)),
    }
}

fn key_input(event: &NSEvent) -> KeyInput {
    // `charactersIgnoringModifiers` applies Shift but not Control/Option/
    // Command, like windows-chewing-tsf's ToAscii call without Ctrl.
    let text = event
        .charactersIgnoringModifiers()
        .and_then(|s| s.to_string().chars().next());
    KeyInput {
        key_code: event.keyCode(),
        text,
        modifiers: modifiers(event.modifierFlags()),
    }
}
