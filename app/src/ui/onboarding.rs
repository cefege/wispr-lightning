use dioxus::prelude::*;

pub fn view() -> Element {
    rsx! {
        div { class: "grid h-screen place-items-center bg-window font-sans text-fg",
            div { class: "w-[560px] overflow-hidden rounded-xl bg-surface shadow-modal",
                header { class: "flex items-center justify-between bg-brand-gradient px-6 py-5 text-white",
                    span { class: "text-[15px] font-semibold", "Wispr Lightning" }
                    span { class: "text-xs", "Step 1 of 3" }
                }
                div { class: "p-8",
                    h1 { class: "text-xl font-semibold", "Approve access" }
                    p { class: "mt-2 text-sm text-fg-muted", "Setup is required before dictation can begin." }
                }
            }
        }
    }
}
