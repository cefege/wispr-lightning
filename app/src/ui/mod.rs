use std::sync::Arc;
use std::path::PathBuf;
use dioxus::prelude::*;

use dioxus::desktop::window;

use crate::bus::Bus;
use crate::windows::Section;
pub mod dictionary;
pub mod format;
pub mod history;
pub mod home;
pub mod icons;
pub mod notes;
pub mod onboarding;
pub mod settings;
pub mod widgets;

pub mod store;

pub fn use_on_focus(mut handler: impl FnMut() + 'static) {
    use dioxus::desktop::tao::event::{Event, WindowEvent};
    use dioxus::desktop::{use_wry_event_handler, window};

    let window_id = window().window.id();
    use_wry_event_handler(move |event, _| {
        if let Event::WindowEvent {
            window_id: event_window,
            event: WindowEvent::Focused(true),
            ..
        } = event
        {
            if *event_window == window_id {
                handler();
            }
        }
    });
}

static TAILWIND: Asset = asset!("/assets/tailwind.css");
static DX_THEME: Asset = asset!("/assets/dx-components-theme.css");

pub fn copy_text(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
        .map_err(|error| error.to_string())
}

pub async fn pick_csv() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .add_filter("CSV", &["csv"])
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf())
}

#[derive(Props, Clone, PartialEq)]
pub struct MainWindowProps {
    pub initial: Section,
}

#[component]
pub fn MainWindow(props: MainWindowProps) -> Element {
    store::provide();
    let store = use_context::<store::SettingsStore>();
    let bus = use_context::<Arc<Bus>>();
    let mut section = use_signal(|| props.initial);

    use_future(move || {
        let mut navigation = bus.navigate.subscribe();
        let mut section = section;
        async move {
            loop {
                match navigation.recv().await {
                    Ok(next) => section.set(next),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    });

    let current = section();
    let settings = store.value.read().clone();
    if !settings.did_complete_onboarding {
        return rsx! {
            document::Stylesheet { href: TAILWIND }
            document::Stylesheet { href: DX_THEME }
            {onboarding::view()}
        };
    }

    rsx! {
        document::Stylesheet { href: TAILWIND }
        document::Stylesheet { href: DX_THEME }
        div { class: "grid h-screen grid-cols-[232px_1fr] bg-window font-sans text-[13px] text-fg",
            aside { class: "flex min-h-0 flex-col border-r border-line bg-sidebar",
                if cfg!(target_os = "macos") { div { class: "h-[38px]", onmousedown: move |_| window().drag() } }
                div { class: "flex h-12 items-center gap-2 px-4",
                    div { class: "grid size-7 place-items-center rounded-lg bg-brand-ink",
                        svg { view_box: "0 0 24 24", class: "size-4", fill: "none", stroke: "white", stroke_width: "2",
                            path { d: "M5 6v12M9 4v16M13 7v10M17 5v14M21 8v8", stroke_linecap: "round" }
                        }
                    }
                    span { class: "text-[15px] font-semibold", "Wispr Lightning" }
                }
                nav { class: "flex flex-1 flex-col gap-4 px-3 py-3",
                    for (group_index, group) in Section::NAV.iter().enumerate() {
                        div { class: "flex flex-col gap-1",
                            if group_index == 1 { p { class: "px-2 pb-1 text-[11px] uppercase tracking-wide text-fg-subtle", "Settings" } }
                            for item in group.iter().copied() {
                                button {
                                    class: if item == current { "flex h-8 items-center gap-2 rounded-md bg-selected px-2 text-left text-accent" } else { "flex h-8 items-center gap-2 rounded-md px-2 text-left text-fg-muted hover:bg-control-hover" },
                                    onclick: move |_| section.set(item),
                                    icons::Icon {
                                        name: match item {
                                            Section::Home => icons::IconName::Home,
                                            Section::History => icons::IconName::History,
                                            Section::Dictionary => icons::IconName::Dictionary,
                                            Section::Notes => icons::IconName::Notes,
                                            Section::General => icons::IconName::General,
                                            Section::Dictation => icons::IconName::Dictation,
                                            Section::Transcription => icons::IconName::Transcription,
                                            Section::Privacy => icons::IconName::Privacy,
                                            Section::System => icons::IconName::System,
                                        },
                                        size: 16
                                    }
                                    span { "{item.title()}" }
                                }
                            }
                        }
                    }
                }
            }
            main { class: "flex min-h-0 flex-col",
                header { class: "flex h-14 items-center border-b border-line px-6", onmousedown: move |_| window().drag(),
                    h1 { class: "text-xl font-semibold", "{current.title()}" }
                }
                section { class: "min-h-0 flex-1 overflow-y-auto px-6 pb-8 pt-5",
                    match current {
                        Section::Home => home::view(),
                        Section::History => history::view(),
                        Section::Dictionary => dictionary::view(),
                        Section::Notes => notes::view(),
                        Section::General => settings::general::view(),
                        Section::Dictation => settings::dictation::view(),
                        Section::Transcription => settings::transcription::view(),
                        Section::Privacy => settings::privacy::view(),
                        Section::System => settings::system::view(),
                    }
                }
            }
        }
    }
}
