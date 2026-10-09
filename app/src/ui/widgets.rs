use dioxus::prelude::*;

use crate::ui::icons::{Icon, IconName};

#[derive(Props, Clone, PartialEq)]
pub struct SettingRowProps {
    pub title: String,
    #[props(default)]
    pub description: Option<String>,
    pub control: Element,
}

#[component]
pub fn SettingRow(props: SettingRowProps) -> Element {
    rsx! {
        div { class: "flex min-h-12 items-center justify-between gap-6 py-3",
            div { class: "min-w-0",
                p { class: "m-0 text-[13px] font-medium", "{props.title}" }
                if let Some(description) = props.description {
                    p { class: "mb-0 mt-1 text-xs text-fg-muted", "{description}" }
                }
            }
            div { class: "shrink-0", {props.control} }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct SearchFieldProps {
    pub value: String,
    #[props(default = "Search".to_string())]
    pub placeholder: String,
    pub oninput: EventHandler<FormEvent>,
}

#[component]
pub fn SearchField(props: SearchFieldProps) -> Element {
    rsx! {
        crate::components::input::Input {
            class: "h-9 w-full rounded-md border border-line bg-control px-3 text-[13px] text-fg placeholder:text-fg-subtle",
            r#type: "search",
            value: props.value,
            placeholder: props.placeholder,
            oninput: props.oninput,
            children: rsx! {}
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct EmptyStateProps {
    pub icon: IconName,
    pub title: String,
    #[props(default)]
    pub action_label: Option<String>,
    #[props(default)]
    pub on_action: Option<EventHandler<MouseEvent>>,
}

#[component]
pub fn EmptyState(props: EmptyStateProps) -> Element {
    rsx! {
        div { class: "grid justify-items-center gap-3 py-12 text-center",
            div { class: "text-fg-subtle", Icon { name: props.icon, size: 36 } }
            p { class: "m-0 text-sm text-fg-muted", "{props.title}" }
            if let (Some(label), Some(on_action)) = (props.action_label, props.on_action) {
                crate::components::button::Button {
                    variant: crate::components::button::ButtonVariant::Secondary,
                    onclick: move |event| on_action.call(event),
                    "{label}"
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct ErrorBannerProps {
    pub message: String,
    #[props(default)]
    pub on_retry: Option<EventHandler<MouseEvent>>,
}

#[component]
pub fn ErrorBanner(props: ErrorBannerProps) -> Element {
    rsx! {
        div { class: "flex items-center justify-between gap-3 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger", role: "alert",
            span { "{props.message}" }
            if let Some(on_retry) = props.on_retry {
                button { class: "shrink-0 underline", r#type: "button", onclick: move |event| on_retry.call(event), "Retry" }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct KeyCaptureProps {
    pub label: String,
    pub capturing: bool,
    pub onclick: EventHandler<MouseEvent>,
}

#[component]
pub fn KeyCapture(props: KeyCaptureProps) -> Element {
    rsx! {
        button {
            class: if props.capturing { "min-h-9 rounded-md border border-accent bg-selected px-3 text-sm text-accent" } else { "min-h-9 rounded-md border border-line bg-control px-3 text-sm text-fg hover:bg-control-hover" },
            r#type: "button",
            onclick: props.onclick,
            if props.capturing { "Press a key…" } else { "{props.label}" }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct ListRowProps {
    pub children: Element,
    pub onactivate: EventHandler<()>,
}

#[component]
pub fn ListRow(props: ListRowProps) -> Element {
    rsx! {
        div {
            class: "rounded-lg border border-line bg-surface shadow-card focus-visible:outline-2 focus-visible:outline-accent",
            role: "button",
            tabindex: "0",
            onclick: move |_| props.onactivate.call(()),
            onkeydown: move |event| {
                let key = event.key();
                if key == Key::Enter || key == Key::Character(" ".to_string()) {
                    event.prevent_default();
                    props.onactivate.call(());
                }
            },
            {props.children}
        }
    }
}
