use crate::ui::{store::SettingsStore, widgets::SettingRow};
use dioxus::prelude::*;
use wl_core::settings::{EmailSignature, TypingSpeed};

#[component]
pub fn view() -> Element {
    let store = use_context::<SettingsStore>();
    let value = store.value.read().clone();
    let speed = match value.natural_mode_speed {
        TypingSpeed::Slow => "slow",
        TypingSpeed::Normal => "normal",
        TypingSpeed::Expert => "expert",
    };
    let signature = match &value.email_signature_option {
        EmailSignature::WrittenWithLightning => "written_with_lightning",
        EmailSignature::SpokenWithLightning => "spoken_with_lightning",
    };
    rsx! {
        div { class: "mx-auto max-w-[760px] space-y-4",
            div { class: "rounded-lg border border-line bg-surface p-4 shadow-card",
                h2 { class: "mb-2 text-sm font-semibold", "Transcript behavior" }
                SettingRow { title: "Voice commands", description: Some("Convert phrases like “new line” and “comma” into layout and punctuation".into()), control: rsx! { crate::components::switch::Switch { checked: value.command_mode_enabled, on_checked_change: move |v| { let mut s=store;s.update(|s|s.command_mode_enabled=v); } } } }
                SettingRow { title: "Auto-learn words", description: Some("Learn distinctive vocabulary from corrected transcripts".into()), control: rsx! { crate::components::switch::Switch { checked: value.auto_learn_words, on_checked_change: move |v| { let mut s=store;s.update(|s|s.auto_learn_words=v); } } } }
                hr { class: "my-2 border-line" }
                SettingRow { title: "Natural Mode", description: Some("Type character by character instead of pasting".into()), control: rsx! { crate::components::switch::Switch { checked: value.natural_mode_enabled, on_checked_change: move |v| { let mut s=store;s.update(|s|s.natural_mode_enabled=v); } } } }
                if value.natural_mode_enabled {
                    div { class: "mt-3 flex items-center justify-between", span { "Typing speed" }
                        select { class: "h-9 rounded-md border border-line bg-control px-2", value: speed, onchange: move |e| { let next=match e.value().as_str(){"slow"=>TypingSpeed::Slow,"expert"=>TypingSpeed::Expert,_=>TypingSpeed::Normal};let mut s=store;s.update(|s|s.natural_mode_speed=next); },
                            option { value: "slow", "Slow" } option { value: "normal", "Normal" } option { value: "expert", "Expert" }
                        }
                    }
                    p { class: "mt-2 text-xs text-fg-muted", "Slow ≈ 30 WPM, Normal ≈ 50 WPM, Expert ≈ 80 WPM" }
                }
            }
            div { class: "rounded-lg border border-line bg-surface p-4 shadow-card",
                h2 { class: "mb-2 text-sm font-semibold", "Email" }
                SettingRow { title: "Email signature", description: Some("Append a short signature when dictating in email apps".into()), control: rsx! { crate::components::switch::Switch { checked: value.email_auto_signature, on_checked_change: move |v| { let mut s=store;s.update(|s|s.email_auto_signature=v); } } } }
                if value.email_auto_signature {
                    div { class: "mt-3 flex items-center justify-between gap-4", label { "Signature" }
                        select { class: "h-9 rounded-md border border-line bg-control px-2", value: signature, onchange: move |e| { let next=if e.value()=="spoken_with_lightning"{EmailSignature::SpokenWithLightning}else{EmailSignature::WrittenWithLightning};let mut s=store;s.update(|s|s.email_signature_option=next); },
                            option { value: "written_with_lightning", "Written with Wispr Lightning" } option { value: "spoken_with_lightning", "Spoken with Wispr Lightning" }
                        }
                    }
                }
            }
        }
    }
}
