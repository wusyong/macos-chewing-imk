// SPDX-License-Identifier: GPL-3.0-or-later

//! The IMKInputController subclass. IMKServer creates one instance per
//! input session (roughly: per text input client). All instances share one
//! [`ChewingCore`](chewing_imk_core::ChewingCore), see [`crate::state`].

use std::cell::Cell;
use std::time::Duration;

use chewing::editor::CharacterForm;
use chewing_imk_core::Action;
use chewing_imk_core::keymap::{KeyInput, ModifierInput, Modifiers};
use log::{debug, warn};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSMenu};
use objc2_foundation::NSUInteger;
use objc2_input_method_kit::IMKInputController;

use crate::{candidate_window, client, menu, notice_window, prefs, state};

thread_local! {
    /// Anchor of the candidate window within the current marked text.
    static LAST_ANCHOR: Cell<usize> = const { Cell::new(0) };
}

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
                // Pick up edits to the settings file made while we were away.
                state::reload_config_if_changed();
                // Ask for the system layout the zhuyin layout is defined on
                // (usually ABC), whatever layout the user picked last.
                if let Some(layout) =
                    state::with_core(|core| core.config().base_keyboard_layout()).flatten()
                {
                    client::override_keyboard(client, layout);
                }
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

        /// Items added to the input menu while we are active.
        #[unsafe(method_id(menu))]
        fn menu(&self) -> Option<Retained<NSMenu>> {
            build_menu()
        }

        #[unsafe(method(toggleLanguageMode:))]
        fn toggle_language_mode(&self, _sender: Option<&AnyObject>) {
            if let Some(actions) = state::with_core(|core| core.toggle_lang_mode()) {
                self.apply_to_current_client(actions);
            }
        }

        #[unsafe(method(toggleShapeMode:))]
        fn toggle_shape_mode(&self, _sender: Option<&AnyObject>) {
            state::with_core(|core| core.toggle_shape_mode());
        }

        #[unsafe(method(toggleSimplifiedChinese:))]
        fn toggle_simplified_chinese(&self, _sender: Option<&AnyObject>) {
            if let Some(actions) = state::with_core(|core| core.toggle_simplified_chinese()) {
                self.apply_to_current_client(actions);
            }
        }

        #[unsafe(method(openPreferences:))]
        fn open_preferences(&self, _sender: Option<&AnyObject>) {
            if let Some(mtm) = MainThreadMarker::new() {
                prefs::show(mtm);
            }
        }

        #[unsafe(method(openLink:))]
        fn open_link(&self, sender: Option<&AnyObject>) {
            menu::open_link(sender);
        }
    }
);

impl ChewingController {
    /// Menu actions are not tied to a key event; use the controller's client.
    fn apply_to_current_client(&self, actions: Vec<Action>) {
        // SAFETY: `client` is an IMKInputController method returning the
        // current IMKTextInput client.
        let client: Option<Retained<AnyObject>> = unsafe { msg_send![self, client] };
        match client {
            Some(client) => self.apply(&client, actions),
            None => warn!("menu action without a client"),
        }
    }

    fn handle_event_impl(&self, event: Option<&NSEvent>, sender: Option<&AnyObject>) -> bool {
        let (Some(event), Some(client)) = (event, sender) else {
            return false;
        };
        let ty = event.r#type();
        let resp = if ty == NSEventType::KeyDown {
            // Cheap (one stat); like windows-chewing-tsf, check every key.
            state::reload_config_if_changed();
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
        // Index (UTF-16) of the character the candidates are for. Moving
        // the candidate highlight does not touch the marked text, so keep the
        // anchor from the last composition update.
        let mut anchor = LAST_ANCHOR.get();
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
                    LAST_ANCHOR.set(anchor);
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
            let (font_size, font_family) = state::with_core(|core| {
                let cfg = core.config();
                (cfg.font_size, cfg.font_family.clone())
            })
            .unwrap_or((16.0, String::new()));
            let style = candidate_window::Style {
                font_size,
                font_family: &font_family,
            };
            candidate_window::show(mtm, &list, &style, line);
        }
        if let Some(message) = notice {
            notice_window::show(mtm, &message, line);
        }
    }
}

fn build_menu() -> Option<Retained<NSMenu>> {
    let mtm = MainThreadMarker::new()?;
    let state = state::with_core(|core| menu::MenuState {
        lang_mode: core.lang_mode(),
        fullwidth: core.character_form() == CharacterForm::Fullwidth,
        simplified: core.output_simp_chinese(),
    })?;
    Some(menu::build(mtm, &state))
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
