use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconName {
    Home,
    Search,
    Plus,
    Copy,
    Trash,
    HistoryEmpty,
    Note,
    Book,
    Snippet,
    Import,
    Warning,
    Close,
    Chevron,
    General,
    History,
    Dictionary,
    Notes,
    Dictation,
    Transcription,
    Privacy,
    System,
}

#[derive(Props, Clone, PartialEq)]
pub struct IconProps {
    pub name: IconName,
    #[props(default = 16)]
    pub size: u32,
}

#[component]
pub fn Icon(props: IconProps) -> Element {
    let stroke = if props.size >= 28 { "1.25" } else { "1.5" };
    rsx! {
        svg {
            class: "block flex-none",
            width: props.size,
            height: props.size,
            view_box: "0 0 16 16",
            fill: "none",
            stroke: "currentColor",
            stroke_width: stroke,
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            match props.name {
                IconName::Search => rsx! {
                    circle { cx: "7", cy: "7", r: "4.5" }
                    path { d: "M10.4 10.4 14 14" }
                },
                IconName::Plus => rsx! { path { d: "M8 3v10M3 8h10" } },
                IconName::Copy => rsx! {
                    rect { x: "5.75", y: "5.75", width: "7.5", height: "8.5", rx: "1.5" }
                    path { d: "M10.25 3.75H4.25a1.5 1.5 0 0 0-1.5 1.5v6" }
                },
                IconName::Trash => rsx! {
                    path { d: "M2.75 4.5h10.5" }
                    path { d: "M6.5 4.5V3.25a.75.75 0 0 1 .75-.75h1.5a.75.75 0 0 1 .75.75V4.5" }
                    path { d: "M4.25 4.5l.55 8.3a1 1 0 0 0 1 .95h4.4a1 1 0 0 0 1-.95l.55-8.3" }
                },
                IconName::HistoryEmpty => rsx! {
                    path { d: "M2 3.75h12M2 7.5h6.5M2 11.25h5" }
                    circle { cx: "11.75", cy: "11.5", r: "3.25" }
                    path { d: "M10.25 11.5h3" }
                },
                IconName::Note => rsx! {
                    rect { x: "2.75", y: "2", width: "10.5", height: "12", rx: "1.75" }
                    path { d: "M5.5 5.5h5M5.5 8h5M5.5 10.5h3" }
                },
                IconName::Book => rsx! {
                    path { d: "M4 2.25h7.25a1 1 0 0 1 1 1v9.5a1 1 0 0 1-1 1H4z" }
                    path { d: "M4 2.25a1.25 1.25 0 0 0 0 11.5" }
                    path { d: "M6.5 5.75h3.25" }
                },
                IconName::Snippet => rsx! {
                    path { d: "M5.5 3.5 2.5 8l3 4.5" }
                    path { d: "M10.5 3.5 13.5 8l-3 4.5" }
                },
                IconName::Import => rsx! {
                    path { d: "M8 2.25v7.5M5.25 7l2.75 2.75L10.75 7" }
                    path { d: "M2.75 11.5v1.25a1 1 0 0 0 1 1h8.5a1 1 0 0 0 1-1V11.5" }
                },
                IconName::Warning => rsx! {
                    path { d: "M7.13 2.6 1.4 12.5a1 1 0 0 0 .87 1.5h11.46a1 1 0 0 0 .87-1.5L8.87 2.6a1 1 0 0 0-1.74 0Z" }
                    path { d: "M8 6.25v3.25" }
                    circle { cx: "8", cy: "11.6", r: "0.6", fill: "currentColor", stroke: "none" }
                },
                IconName::Close => rsx! { path { d: "M4 4l8 8M12 4l-8 8" } },
                IconName::Chevron => rsx! { path { d: "m5 6 3 3 3-3" } },
                IconName::Home => rsx! {
                    path { d: "m2 7.5 6-5 6 5" }
                    path { d: "M3.5 6.5v7h9v-7M6.5 13.5V9h3v4.5" }
                },
                IconName::General => rsx! {
                    g { transform: "scale(0.5714)",
                        circle { cx: "14", cy: "14", r: "4.7" }
                        circle { cx: "14", cy: "14", r: "1.9" }
                        path { d: "M14 7.3v2M14 18.7v2M20.7 14h-2M7.3 14h2M18.74 9.26l-1.42 1.42M10.68 17.32l-1.42 1.42M18.74 18.74l-1.42-1.42M10.68 10.68L9.26 9.26" }
                    }
                },
                IconName::System => rsx! {
                    g { transform: "scale(0.5714)",
                        rect { x: "7", y: "8", width: "14", height: "9.6", rx: "1.6" }
                        path { d: "M11 20.8h6M14 17.6v3.2" }
                    }
                },
                IconName::Dictation => rsx! {
                    g { transform: "scale(0.5714)",
                        rect { x: "11.4", y: "7", width: "5.2", height: "9.6", rx: "2.6", fill: "currentColor" }
                        path { d: "M9 14.2a5 5 0 0 0 10 0M14 19.2v1.8M11.6 21h4.8" }
                    }
                },
                IconName::Transcription => rsx! {
                    g { transform: "scale(0.5714)",
                        path { d: "M7.6 14h1.4M11.6 10.4v7.2M15.2 8v12M18.8 11.6v4.8M22 13.4v1.2" }
                    }
                },
                IconName::Privacy => rsx! {
                    g { transform: "scale(0.5714)",
                        path { d: "M10.4 14.6V9.8a1.3 1.3 0 0 1 2.6 0v3.4V8.2a1.3 1.3 0 0 1 2.6 0v5V9.6a1.3 1.3 0 0 1 2.6 0v6.2a5 5 0 0 1-10 0v-1.6a1.3 1.3 0 0 1 2.2-.9z" }
                    }
                },
                IconName::History => rsx! {
                    g { transform: "scale(0.5714)",
                        circle { cx: "14", cy: "14", r: "6.4" }
                        path { d: "M14 10.2V14l2.6 1.8" }
                    }
                },
                IconName::Dictionary => rsx! {
                    g { transform: "scale(0.5714)",
                        path { d: "M8.4 8.2h8.4a2.4 2.4 0 0 1 2.4 2.4v9.2H10.8a2.4 2.4 0 0 1-2.4-2.4z" }
                        path { d: "M8.4 17.4a2.4 2.4 0 0 1 2.4-2.4h8.4M13 11.6h3.2" }
                    }
                },
                IconName::Notes => rsx! {
                    g { transform: "scale(0.5714)",
                        rect { x: "8.2", y: "7.4", width: "11.6", height: "13.2", rx: "1.8" }
                        path { d: "M11 11h6M11 14h6M11 17h3.4" }
                    }
                },
            }
        }
    }
}
