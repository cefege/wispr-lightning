use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use wl_core::settings::Hotkey;
use wl_shell::{ops, state::AppState};

use crate::components::button::{Button, ButtonVariant};
use crate::components::input::Input;
use crate::ui::{store::SettingsStore, widgets::ErrorBanner};

const MACOS_PERMISSIONS: [(&str, &str, &str); 4] = [
    ("microphone", "Microphone", "So it can hear you."),
    (
        "input_monitoring",
        "Input Monitoring",
        "So it can see the key you hold.",
    ),
    (
        "accessibility",
        "Accessibility",
        "So it can type into the app in front of you.",
    ),
    (
        "screen_recording",
        "Screen Recording",
        "So it can recognize names and technical terms visible on screen.",
    ),
];
const WINDOWS_PERMISSIONS: [(&str, &str, &str); 1] =
    [("microphone", "Microphone", "So it can hear you.")];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Permissions,
    Hotkey,
    Deepgram,
}

impl Step {
    fn title(self) -> &'static str {
        match self {
            Self::Permissions => "Approve access",
            Self::Hotkey => "Your dictation key",
            Self::Deepgram => "Connect Deepgram",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Permissions => 0,
            Self::Hotkey => 1,
            Self::Deepgram => 2,
        }
    }
}

#[derive(Clone)]
struct CaptureCleanup(Arc<AppState>);

impl Drop for CaptureCleanup {
    fn drop(&mut self) {
        self.0.cancel_hotkey_capture();
    }
}

fn permission_state(statuses: &BTreeMap<String, &'static str>, key: &str) -> Option<&'static str> {
    statuses.get(key).copied()
}

#[component]
pub fn OnboardingFlow() -> Element {
    let state = use_context::<Arc<AppState>>();
    let mut store = use_context::<SettingsStore>();
    let mut step = use_signal(|| Step::Permissions);
    let permissions = use_signal(BTreeMap::<String, &'static str>::new);
    let permission_error = use_signal(|| None::<String>);
    let permissions_loaded = use_signal(|| false);
    let mut active_permission = use_signal(|| None::<String>);
    let mut requested = use_signal(Vec::<String>::new);
    let mut recording = use_signal(|| false);
    let deepgram_status = use_signal(|| None::<bool>);
    let deepgram_key = use_signal(String::new);
    let mut deepgram_busy = use_signal(|| false);
    let mut deepgram_error = use_signal(|| None::<String>);
    let mut finishing = use_signal(|| false);
    let mut finish_error = use_signal(|| None::<String>);
    let settings = store.value.read().clone();
    let current_step = step();

    // This guard also covers leaving the step while the polling future is
    // cancelled, ensuring a capture cannot remain armed in the backend.
    let _capture_cleanup = use_hook({
        let state = Arc::clone(&state);
        move || CaptureCleanup(state)
    });

    let required: Vec<(&'static str, &'static str, &'static str)> = if cfg!(target_os = "windows") {
        WINDOWS_PERMISSIONS.to_vec()
    } else {
        MACOS_PERMISSIONS
            .iter()
            .copied()
            .filter(|(key, _, _)| *key != "screen_recording" || settings.use_screen_context)
            .collect()
    };

    let rows = required
        .iter()
        .filter_map(|(key, name, why)| {
            let status = permission_state(&permissions.read(), key)?;
            (status != "not_applicable").then_some((*key, *name, *why, status))
        })
        .collect::<Vec<_>>();
    let ready = *permissions_loaded.read()
        && permission_error.read().is_none()
        && rows.iter().all(|(_, _, _, status)| *status == "granted");
    let deepgram_ready = *deepgram_status.read() == Some(true);
    let can_advance = match current_step {
        Step::Permissions => ready,
        Step::Hotkey => true,
        Step::Deepgram => deepgram_ready,
    };

    let required_for_poll = required.clone();

    // Poll immediately and then every second while the permissions step is
    // active, because native permission decisions do not send UI events.
    let step_for_poll = step;
    use_future({
        let state = Arc::clone(&state);
        move || {
            let state = Arc::clone(&state);
            let mut permissions = permissions;
            let mut permission_error = permission_error;
            let mut permissions_loaded = permissions_loaded;
            let mut active_permission = active_permission;
            let mut requested = requested;
            let required = required_for_poll.clone();
            let step = step_for_poll;
            async move {
                loop {
                    if *step.read() != Step::Permissions {
                        tokio::time::sleep(Duration::from_millis(1000)).await;
                        continue;
                    }
                    let state_for_query = Arc::clone(&state);
                    let result = tokio::task::spawn_blocking(move || {
                        ops::permissions_status(&state_for_query)
                    })
                    .await
                    .map_err(|error| format!("{error}"))
                    .and_then(|result| result);
                    match result {
                        Ok(statuses) => {
                            permissions.set(statuses);
                            permission_error.set(None);
                        }
                        Err(error) => permission_error.set(Some(error)),
                    }
                    permissions_loaded.set(true);

                    let statuses = permissions.read().clone();
                    if permission_error.read().is_none() && !ready_for(&required, &statuses) {
                        let active = active_permission.read().clone();
                        let active_still_pending = active.as_ref().is_some_and(|key| {
                            statuses.get(key).is_some_and(|status| *status == "not_determined")
                        });
                        if !active_still_pending {
                            if let Some((key, _, _)) = required.iter().find(|(key, _, _)| {
                                statuses.get(*key).is_some_and(|status| *status == "not_determined")
                            }) {
                                if !requested.read().contains(&key.to_string()) {
                                    active_permission.set(Some(key.to_string()));
                                    requested.write().push(key.to_string());
                                    let state_for_request = Arc::clone(&state);
                                    let key = key.to_string();
                                    let result = tokio::task::spawn_blocking(move || {
                                        ops::permissions_request(&state_for_request, &key)
                                    })
                                    .await
                                    .map_err(|error| error.to_string())
                                    .and_then(|result| result);
                                    if let Err(error) = result {
                                        permission_error.set(Some(error));
                                    }
                                }
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
            }
        }
    });

    // Status is loaded on mount; refresh once more after each save to reflect
    // the masked configured state.
    use_future({
        let state = Arc::clone(&state);
        move || {
            let state = Arc::clone(&state);
            let mut deepgram_status = deepgram_status;
            let mut deepgram_error = deepgram_error;
            async move {
                let state_for_query = Arc::clone(&state);
                match tokio::task::spawn_blocking(move || ops::deepgram_status(&state_for_query))
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|result| result)
                {
                    Ok(status) => {
                        deepgram_status.set(Some(status.configured));
                        deepgram_error.set(None);
                    }
                    Err(error) => deepgram_error.set(Some(error)),
                }
            }
        }
    });

    let state_for_next = Arc::clone(&state);
    let mut next = move |_| {
        if !can_advance || *finishing.read() {
            return;
        }
        match current_step {
            Step::Permissions => step.set(Step::Hotkey),
            Step::Hotkey => {
                state_for_next.cancel_hotkey_capture();
                recording.set(false);
                step.set(Step::Deepgram);
            }
            Step::Deepgram => {
                finishing.set(true);
                finish_error.set(None);
                spawn(async move {
                    match store.complete_onboarding().await {
                        Ok(()) => {}
                        Err(error) => finish_error.set(Some(error)),
                    }
                    finishing.set(false);
                });
            }
        }
    };

    let state_for_keyboard = Arc::clone(&state);
    let on_keydown = move |event: KeyboardEvent| {
        if event.key() != Key::Enter
            || event.is_auto_repeating()
            || event.modifiers().contains(Modifiers::ALT)
            || event.modifiers().contains(Modifiers::CONTROL)
            || event.modifiers().contains(Modifiers::META)
            || event.modifiers().contains(Modifiers::SHIFT)
            || *recording.read()
            || !can_advance
            || *finishing.read()
        {
            return;
        }
        event.prevent_default();
        match current_step {
            Step::Permissions => step.set(Step::Hotkey),
            Step::Hotkey => {
                state_for_keyboard.cancel_hotkey_capture();
                recording.set(false);
                step.set(Step::Deepgram);
            }
            Step::Deepgram => {
                finishing.set(true);
                finish_error.set(None);
                let mut store = store;
                spawn(async move {
                    if let Err(error) = store.complete_onboarding().await {
                        finish_error.set(Some(error));
                    }
                    finishing.set(false);
                });
            }
        }
    };

    let state_for_permission_request = Arc::clone(&state);
    let state_for_permission_settings = Arc::clone(&state);
    let state_for_hotkey_capture = Arc::clone(&state);
    let state_for_deepgram_save = Arc::clone(&state);
    let state_for_back = Arc::clone(&state);
    let state_for_permission_retry = Arc::clone(&state);
    let state_for_deepgram_refresh = Arc::clone(&state);

    let step_index = current_step.index() + 1;
    rsx! {
        div {
            class: "grid h-screen place-items-center bg-window px-6 font-sans text-[13px] text-fg",
            onkeydown: on_keydown,
            div { class: "flex max-h-full w-[560px] max-w-full flex-col overflow-hidden rounded-xl bg-surface shadow-modal",
                header { class: "flex shrink-0 items-center justify-between bg-brand-gradient px-6 py-4 text-white",
                    div { class: "flex items-center gap-3",
                        div { class: "grid size-7 place-items-center rounded-lg bg-brand-ink",
                            svg { view_box: "0 0 24 24", class: "size-4", fill: "none", stroke: "white", stroke_width: "2",
                                path { d: "M5 6v12M9 4v16M13 7v10M17 5v14M21 8v8", stroke_linecap: "round" }
                            }
                        }
                        span { class: "text-[15px] font-semibold", "Wispr Lightning" }
                    }
                    span { class: "text-xs", "Step {step_index} of 3" }
                }
                div { class: "flex min-h-0 flex-1 flex-col p-7",
                    h1 { class: "shrink-0 text-xl font-semibold", "{current_step.title()}" }
                    div { class: "flex min-h-0 flex-1 flex-col items-start gap-4 overflow-y-auto py-4",
                        match current_step {
                            Step::Permissions => rsx! {
                                PermissionStep {
                                    rows,
                                    permissions_loaded: *permissions_loaded.read(),
                                    active_permission: active_permission.read().clone(),
                                    error: permission_error.read().clone(),
                                    on_retry: move |_| {
                                        let state = Arc::clone(&state_for_permission_retry);
                                        let mut statuses = permissions;
                                        let mut error = permission_error;
                                        let mut loaded = permissions_loaded;
                                        spawn(async move {
                                            match tokio::task::spawn_blocking(move || ops::permissions_status(&state))
                                                .await.map_err(|e| e.to_string()).and_then(|r| r)
                                            {
                                                Ok(next) => { statuses.set(next); error.set(None); }
                                                Err(message) => error.set(Some(message)),
                                            }
                                            loaded.set(true);
                                        });
                                    },
                                    on_request: move |key: String| {
                                        active_permission.set(Some(key.clone()));
                                        if !requested.read().contains(&key) {
                                            requested.write().push(key.clone());
                                        }
                                        let state = Arc::clone(&state_for_permission_request);
                                        let mut error = permission_error;
                                        let mut statuses = permissions;
                                        let mut loaded = permissions_loaded;
                                        spawn(async move {
                                            let key_for_op = key.clone();
                                            let state_for_op = Arc::clone(&state);
                                            let result = tokio::task::spawn_blocking(move || {
                                                ops::permissions_request(&state_for_op, &key_for_op)
                                            }).await.map_err(|e| e.to_string()).and_then(|r| r);
                                            match result {
                                                Ok(()) => error.set(None),
                                                Err(message) => error.set(Some(message)),
                                            }
                                            let state_for_status = Arc::clone(&state);
                                            match tokio::task::spawn_blocking(move || ops::permissions_status(&state_for_status))
                                                .await.map_err(|e| e.to_string()).and_then(|r| r)
                                            {
                                                Ok(next) => { statuses.set(next); error.set(None); }
                                                Err(message) => error.set(Some(message)),
                                            }
                                            loaded.set(true);
                                        });
                                    },
                                    on_open_settings: move |key: String| {
                                        let state = Arc::clone(&state_for_permission_settings);
                                        let mut error = permission_error;
                                        let mut statuses = permissions;
                                        spawn(async move {
                                            let key_for_op = key.clone();
                                            let state_for_op = Arc::clone(&state);
                                            let result = tokio::task::spawn_blocking(move || {
                                                ops::permissions_open_settings(&state_for_op, &key_for_op)
                                            }).await.map_err(|e| e.to_string()).and_then(|result| result);
                                            match result {
                                                Ok(()) => error.set(None),
                                                Err(message) => error.set(Some(message)),
                                            }
                                            let state_for_status = Arc::clone(&state);
                                            match tokio::task::spawn_blocking(move || ops::permissions_status(&state_for_status))
                                                .await.map_err(|e| e.to_string()).and_then(|r| r)
                                            {
                                                Ok(next) => { statuses.set(next); error.set(None); }
                                                Err(message) => error.set(Some(message)),
                                            }
                                        });
                                    }
                                }
                            },
                            Step::Hotkey => rsx! {
                                HotkeyStep {
                                    hotkeys: settings.hotkeys.clone(),
                                    capturing: *recording.read(),
                                    on_hotkeys: move |hotkeys: Vec<Hotkey>| {
                                        store.update(|value| value.hotkeys = hotkeys);
                                    },
                                    on_capture: move |_| {
                                        if *recording.read() {
                                            recording.set(false);
                                            state_for_hotkey_capture.cancel_hotkey_capture();
                                        } else {
                                            recording.set(true);
                                            state_for_hotkey_capture.begin_hotkey_capture();
                                            let state = Arc::clone(&state_for_hotkey_capture);
                                            let mut recording = recording;
                                            let mut store = store;
                                            spawn(async move {
                                                let started = tokio::time::Instant::now();
                                                loop {
                                                    if started.elapsed() >= Duration::from_secs(15) {
                                                        state.cancel_hotkey_capture();
                                                        recording.set(false);
                                                        break;
                                                    }
                                                    tokio::time::sleep(Duration::from_millis(120)).await;
                                                    if !*recording.read() { break; }
                                                    if let Some(hotkey) = state.end_hotkey_capture() {
                                                        let mut hotkeys = store.value.read().hotkeys.clone();
                                                        if !hotkeys.contains(&hotkey) {
                                                            hotkeys.push(hotkey);
                                                            store.update(|value| value.hotkeys = hotkeys);
                                                        }
                                                        recording.set(false);
                                                        break;
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            },
                            Step::Deepgram => rsx! {
                                DeepgramStep {
                                    configured: deepgram_ready,
                                    api_key: deepgram_key.read().clone(),
                                    busy: *deepgram_busy.read(),
                                    error: deepgram_error.read().clone(),
                                    on_retry: move |_| {
                                        let state = Arc::clone(&state_for_deepgram_refresh);
                                        let mut status = deepgram_status;
                                        let mut error = deepgram_error;
                                        spawn(async move {
                                            match tokio::task::spawn_blocking(move || ops::deepgram_status(&state))
                                                .await.map_err(|e| e.to_string()).and_then(|result| result)
                                            {
                                                Ok(result) => { status.set(Some(result.configured)); error.set(None); }
                                                Err(message) => error.set(Some(message)),
                                            }
                                        });
                                    },
                                    on_key: {
                                        let mut key = deepgram_key;
                                        move |event: FormEvent| key.set(event.value())
                                    },
                                    on_save: move |_| {
                                        let key = deepgram_key.read().trim().to_string();
                                        if key.is_empty() || *deepgram_busy.read() { return; }
                                        deepgram_busy.set(true);
                                        deepgram_error.set(None);
                                        let state = Arc::clone(&state_for_deepgram_save);
                                        let mut key_signal = deepgram_key;
                                        let mut status = deepgram_status;
                                        let mut error = deepgram_error;
                                        let mut busy = deepgram_busy;
                                        spawn(async move {
                                            let key_to_save = key.clone();
                                            let state_for_op = Arc::clone(&state);
                                            let result = tokio::task::spawn_blocking(move || {
                                                ops::deepgram_key_save(&state_for_op, &key_to_save)
                                            }).await.map_err(|error| error.to_string()).and_then(|result| result);
                                            match result {
                                                Ok(()) => {
                                                    key_signal.set(String::new());
                                                    let state_for_status = Arc::clone(&state);
                                                    match tokio::task::spawn_blocking(move || ops::deepgram_status(&state_for_status))
                                                        .await.map_err(|e| e.to_string()).and_then(|r| r)
                                                    {
                                                        Ok(result) => {
                                                            status.set(Some(result.configured));
                                                            error.set(None);
                                                        }
                                                        Err(message) => error.set(Some(message)),
                                                    }
                                                }
                                                Err(message) => error.set(Some(message)),
                                            }
                                            busy.set(false);
                                        });
                                    }
                                }
                            },
                        }
                    }
                    footer { class: "flex shrink-0 flex-col gap-3 border-t border-line pt-4",
                        if let Some(message) = store.save_error.read().clone() {
                            ErrorBanner { message: format!("Could not save settings: {message}") }
                        }
                        if let Some(message) = finish_error.read().clone() {
                            ErrorBanner { message }
                        }
                        div { class: "flex items-center gap-2",
                            div { class: "flex-1" }
                            if step_index > 1 {
                                Button {
                                    variant: ButtonVariant::Secondary,
                                    onkeydown: move |event: KeyboardEvent| event.stop_propagation(),
                                    onclick: move |_| {
                                        if current_step == Step::Deepgram { step.set(Step::Hotkey); }
                                        else { step.set(Step::Permissions); }
                                        recording.set(false);
                                        state_for_back.cancel_hotkey_capture();
                                    },
                                    "Back"
                                }
                            }
                            Button {
                                variant: ButtonVariant::Primary,
                                disabled: *finishing.read() || !can_advance,
                                onkeydown: move |event: KeyboardEvent| event.stop_propagation(),
                                onclick: move |_| next(()),
                                if step_index == 3 { if *finishing.read() { "Finishing…" } else { "Done" } }
                                else { "Continue" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn ready_for(
    required: &[(&'static str, &'static str, &'static str)],
    statuses: &BTreeMap<String, &'static str>,
) -> bool {
    required.iter().all(|(key, _, _)| {
        statuses
            .get(*key)
            .is_some_and(|status| *status == "granted" || *status == "not_applicable")
    })
}

#[derive(Props, Clone, PartialEq)]
struct PermissionStepProps {
    rows: Vec<(&'static str, &'static str, &'static str, &'static str)>,
    permissions_loaded: bool,
    active_permission: Option<String>,
    error: Option<String>,
    on_retry: EventHandler<MouseEvent>,
    on_request: EventHandler<String>,
    on_open_settings: EventHandler<String>,
}

#[component]
fn PermissionStep(props: PermissionStepProps) -> Element {
    let on_retry = props.on_retry;
    let windows = cfg!(target_os = "windows");
    let ready = props.permissions_loaded
        && props.error.is_none()
        && props.rows.iter().all(|(_, _, _, state)| *state == "granted");
    rsx! {
        p { class: "m-0 text-fg",
            if windows {
                "Approve microphone access to continue. Windows Settings will open if access was previously denied."
            } else {
                "Approve each macOS request in order. Setup continues only after every permission required by your current configuration is granted."
            }
        }
        if let Some(error) = &props.error {
            div { onkeydown: move |event| event.stop_propagation(),
                ErrorBanner { message: error.clone(), on_retry: Some(on_retry) }
            }
        }
        if props.rows.is_empty() {
            p { class: "m-0 text-xs text-fg-muted",
                if props.permissions_loaded { "Nothing to grant on this system." } else { "Checking permissions…" }
            }
        } else {
            ul { class: "m-0 flex w-full list-none flex-col gap-3 p-0",
                for (key, name, why, state) in props.rows.iter().copied() {
                    {
                        let is_active = props.active_permission.as_deref() == Some(key);
                        let label = if state == "granted" {
                            "Granted"
                        } else if !is_active {
                            "Waiting"
                        } else if state == "denied" {
                            "Needs approval"
                        } else {
                            "Awaiting decision"
                        };
                        let state_class = match state {
                            "granted" => "text-xs text-success",
                            "denied" if is_active => "text-xs text-danger",
                            "not_determined" if is_active => "text-xs text-warning",
                            _ => "text-xs text-fg-muted",
                        };
                        let on_request = props.on_request;
                        let on_open_settings = props.on_open_settings;
                        rsx! {
                            li { class: "flex items-center gap-3",
                                span { class: "flex min-w-0 flex-1 flex-col gap-0.5",
                                    span { class: "text-[13px]", "{name}" }
                                    span { class: "text-xs text-fg-muted", "{why}" }
                                }
                                span { class: "{state_class}", "{label}" }
                                if is_active && state == "not_determined" {
                                    Button {
                                        variant: ButtonVariant::Secondary,
                                        onkeydown: move |event: KeyboardEvent| event.stop_propagation(),
                                        onclick: move |_| on_request.call(key.to_string()),
                                        "Request Access"
                                    }
                                } else if is_active && state == "denied" {
                                    Button {
                                        variant: ButtonVariant::Secondary,
                                        onkeydown: move |event: KeyboardEvent| event.stop_propagation(),
                                        onclick: move |_| on_open_settings.call(key.to_string()),
                                        "Open Settings"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        p { class: "m-0 text-xs text-fg-muted", role: "status",
            if ready { "All required permissions are granted. Continue to choose your dictation key." }
            else if props.permissions_loaded { "Finish the highlighted permission to unlock the next request." }
            else { "Checking permissions…" }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct HotkeyStepProps {
    hotkeys: Vec<Hotkey>,
    capturing: bool,
    on_hotkeys: EventHandler<Vec<Hotkey>>,
    on_capture: EventHandler<MouseEvent>,
}

#[component]
fn HotkeyStep(props: HotkeyStepProps) -> Element {
    let on_capture = props.on_capture;
    rsx! {
        p { class: "m-0 text-fg", "Hold this key while you talk. Let go, and what you said is typed where your cursor is." }
        div { class: "flex w-full flex-col gap-2", role: "group", "aria-label": "Dictation hotkeys",
            for (index, hotkey) in props.hotkeys.iter().enumerate() {
                {
                    let all_hotkeys = props.hotkeys.clone();
                    let on_hotkeys = props.on_hotkeys;
                    rsx! {
                        div { class: "flex items-center gap-2",
                            span { class: "flex-1 rounded-md border border-line bg-control px-3 py-2 text-sm", "{hotkey.label()}" }
                            if props.hotkeys.len() > 1 {
                                Button {
                                    variant: ButtonVariant::Ghost,
                                    onkeydown: move |event: KeyboardEvent| event.stop_propagation(),
                                    title: "Remove this hotkey",
                                    onclick: move |_| {
                                        let mut hotkeys = all_hotkeys.clone();
                                        hotkeys.remove(index);
                                        on_hotkeys.call(hotkeys);
                                    },
                                    "Remove"
                                }
                            }
                        }
                    }
                }
            }
        }
        Button {
            variant: if props.capturing { ButtonVariant::Outline } else { ButtonVariant::Secondary },
            onkeydown: move |event: KeyboardEvent| event.stop_propagation(),
            onclick: move |event| on_capture.call(event),
            if props.capturing { "Press a key…" } else { "Choose a Different Key" }
        }
        if props.capturing {
            p { class: "m-0 text-xs text-fg-muted", "Press the key you want. For a combination, hold the modifiers and press the other key. Click again to cancel." }
        }
        p { class: "m-0 text-xs text-fg-muted", "Keep the default if it suits you. Extra keys, and what a quick tap does, are in Settings." }
    }
}


#[derive(Props, Clone, PartialEq)]
struct DeepgramStepProps {
    configured: bool,
    api_key: String,
    busy: bool,
    error: Option<String>,
    on_retry: EventHandler<MouseEvent>,
    on_key: EventHandler<FormEvent>,
    on_save: EventHandler<()>,
}

#[component]
fn DeepgramStep(props: DeepgramStepProps) -> Element {
    let on_retry = props.on_retry;
    let on_key = props.on_key;
    let on_save_for_input = props.on_save;
    let on_save_for_button = props.on_save;
    rsx! {
        p { class: "m-0 text-fg", "Wispr Lightning uses Deepgram for fast, live speech recognition." }
        p { class: "m-0 text-xs text-fg-muted", "Paste a Deepgram API key to finish setup. You can replace it later in Settings › Transcription." }
        if let Some(error) = &props.error {
            div { onkeydown: move |event: KeyboardEvent| event.stop_propagation(),
                ErrorBanner { message: error.clone(), on_retry: Some(on_retry) }
            }
        }
        div { class: "grid w-full gap-2",
            label { class: "text-[13px] font-medium", r#for: "onboarding-deepgram-key", "Deepgram API key" }
            div { class: "flex items-center gap-2",
                Input {
                    r#type: "password",
                    id: "onboarding-deepgram-key",
                    autocomplete: "off",
                    class: "min-w-0 flex-1",
                    placeholder: if props.configured { "Saved key ••••••••••••" } else { "Paste Deepgram API key" },
                    value: "{props.api_key}",
                    oninput: move |event| on_key.call(event),
                    onkeydown: move |event: KeyboardEvent| {
                        event.stop_propagation();
                        if event.key() == Key::Enter {
                            event.prevent_default();
                            on_save_for_input.call(());
                        }
                    }
                }
                Button {
                    variant: ButtonVariant::Primary,
                    disabled: props.busy || props.api_key.trim().is_empty(),
                    onkeydown: move |event: KeyboardEvent| event.stop_propagation(),
                    onclick: move |_| on_save_for_button.call(()),
                    if props.configured { "Replace" } else { "Save key" }
                }
            }
        }
        if props.configured {
            p { class: "m-0 text-xs text-success", role: "status", "Deepgram API key saved. Setup is ready to finish." }
        } else {
            p { class: "m-0 text-xs text-fg-muted", "The key is write-only: after saving, the app shows a masked saved state instead of revealing it." }
        }
    }
}

pub fn view() -> Element {
    rsx! { OnboardingFlow {} }
}
