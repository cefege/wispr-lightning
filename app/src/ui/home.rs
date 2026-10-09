use chrono::{Local, NaiveTime, TimeZone};
use dioxus::prelude::*;
use std::sync::Arc;

use crate::bus::{Bus, HISTORY_CHANGED};
use crate::ui::{
    copy_text, format,
    icons::{Icon, IconName},
    use_on_focus,
    widgets::EmptyState,
};
use crate::windows::Section;
use wl_core::db::models::TranscriptEntry;
use wl_shell::{ops, state::AppState};

#[derive(Clone)]
struct DashboardData {
    totals: Option<(i64, i64)>,
    recent: Vec<TranscriptEntry>,
    configured: bool,
    history_error: Option<String>,
}

async fn load_dashboard(state: Arc<AppState>) -> DashboardData {
    let since = Local::now()
        .date_naive()
        .and_time(NaiveTime::MIN)
        .and_local_timezone(Local)
        .earliest()
        .map(|midnight| midnight.timestamp() as f64)
        .unwrap_or_else(|| Local::now().timestamp() as f64);
    let state_for_data = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let totals = ops::history_totals_since(&state_for_data, since)?;
        let recent = ops::history_list(&state_for_data, 5, 0)?;
        Ok::<_, String>((totals, recent))
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(|result| result);
    let configured = tokio::task::spawn_blocking(move || {
        ops::deepgram_status(&state).map(|status| status.configured)
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(|result| result)
    .unwrap_or(false);

    match result {
        Ok((totals, recent)) => DashboardData {
            totals: Some((totals.dictations, totals.words)),
            recent,
            configured,
            history_error: None,
        },
        Err(error) => DashboardData {
            totals: None,
            recent: Vec::new(),
            configured,
            history_error: Some(error),
        },
    }
}

async fn load_balance(state: Arc<AppState>) -> Result<String, String> {
    let balance = ops::deepgram_balance(&state).await?;
    Ok(format::balance(balance.amount, &balance.units))
}

fn apply_dashboard(
    result: DashboardData,
    mut totals: Signal<Option<(i64, i64)>>,
    mut recent: Signal<Vec<TranscriptEntry>>,
    mut configured: Signal<bool>,
    mut history_error: Signal<Option<String>>,
) {
    totals.set(result.totals);
    recent.set(result.recent);
    configured.set(result.configured);
    history_error.set(result.history_error);
}

pub fn view() -> Element {
    let state = use_context::<Arc<AppState>>();
    let bus = use_context::<Arc<Bus>>();
    let settings_store = use_context::<crate::ui::store::SettingsStore>();
    let settings = settings_store.value.read().clone();
    let totals = use_signal(|| None::<(i64, i64)>);
    let recent = use_signal(Vec::<TranscriptEntry>::new);
    let configured = use_signal(|| false);
    let history_error = use_signal(|| None::<String>);
    let mut balance = use_signal(|| None::<String>);
    let mut balance_error = use_signal(|| None::<String>);
    let mut copy_error = use_signal(|| None::<String>);

    let initial_state = Arc::clone(&state);
    let initial_totals = totals;
    let initial_recent = recent;
    let initial_configured = configured;
    let initial_history_error = history_error;
    let initial_balance = balance;
    let initial_balance_error = balance_error;
    use_future(move || {
        let state = Arc::clone(&initial_state);
        let totals = initial_totals;
        let recent = initial_recent;
        let configured = initial_configured;
        let history_error = initial_history_error;
        let mut balance = initial_balance;
        let mut balance_error = initial_balance_error;
        async move {
            apply_dashboard(
                load_dashboard(Arc::clone(&state)).await,
                totals,
                recent,
                configured,
                history_error,
            );
            match load_balance(state).await {
                Ok(value) => {
                    balance.set(Some(value));
                    balance_error.set(None);
                }
                Err(error) => {
                    balance.set(None);
                    balance_error.set(Some(error));
                }
            }
        }
    });

    let event_state = Arc::clone(&state);
    let event_bus = Arc::clone(&bus);
    use_future(move || {
        let mut changed = event_bus.changed.subscribe();
        let state = Arc::clone(&event_state);
        async move {
            loop {
                match changed.recv().await {
                    Ok(HISTORY_CHANGED) => apply_dashboard(
                        load_dashboard(Arc::clone(&state)).await,
                        totals,
                        recent,
                        configured,
                        history_error,
                    ),
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    });

    let settings_bus = Arc::clone(&bus);
    let settings_state = Arc::clone(&state);
    use_future(move || {
        let state = Arc::clone(&settings_state);
        let mut changes = settings_bus.settings.subscribe();
        async move {
            loop {
                if changes.changed().await.is_err() {
                    return;
                }
                apply_dashboard(
                    load_dashboard(Arc::clone(&state)).await,
                    totals,
                    recent,
                    configured,
                    history_error,
                );
            }
        }
    });

    let focus_state = Arc::clone(&state);
    let focus_balance_state = Arc::clone(&state);
    use_on_focus(move || {
        let state = Arc::clone(&focus_state);
        spawn(async move {
            apply_dashboard(
                load_dashboard(state).await,
                totals,
                recent,
                configured,
                history_error,
            );
        });
        let state = Arc::clone(&focus_balance_state);
        spawn(async move {
            match load_balance(state).await {
                Ok(value) => {
                    balance.set(Some(value));
                    balance_error.set(None);
                }
                Err(error) => {
                    balance.set(None);
                    balance_error.set(Some(error));
                }
            }
        });
    });

    let hotkeys = settings.hotkeys.iter().map(|hotkey| hotkey.label()).collect::<Vec<_>>();
    let primary_hotkey = hotkeys.first().cloned().unwrap_or_else(|| "Unset".to_string());
    let other_hotkeys = hotkeys.iter().skip(1).cloned().collect::<Vec<_>>().join(", ");
    let totals_value = totals();
    let recent_entries = recent();
    let resume_state = Arc::clone(&state);
    let retry_state = Arc::clone(&state);
    let setup_bus = Arc::clone(&bus);
    let history_bus = Arc::clone(&bus);

    rsx! {
        div { class: "mx-auto flex max-w-[1000px] flex-col gap-5",
            section { class: "relative overflow-hidden rounded-xl bg-brand-gradient p-6 text-white shadow-card",
                div { class: "absolute right-6 top-5 opacity-15", Icon { name: IconName::Home, size: 72 } }
                p { class: "m-0 text-sm font-medium text-white/75", "Wispr Lightning" }
                h2 { class: "mb-0 mt-2 text-2xl font-semibold", "Hold {primary_hotkey} to dictate" }
                div { class: "mt-2 flex items-center gap-3 text-sm text-white/80",
                    if settings.hotkey_paused {
                        span { "Hotkey paused" }
                        button {
                            class: "rounded-md bg-white/15 px-3 py-1.5 font-medium text-white hover:bg-white/25",
                            r#type: "button",
                            onclick: move |_| {
                                let state = Arc::clone(&resume_state);
                                spawn(async move {
                                    let _ = tokio::task::spawn_blocking(move || ops::hotkey_set_paused(&state, false)).await;
                                });
                            },
                            "Resume"
                        }
                    } else if !other_hotkeys.is_empty() {
                        span { "or {other_hotkeys}" }
                    }
                }
            }

            div { class: "grid grid-cols-3 gap-4",
                div { class: "rounded-lg border border-line bg-surface p-4 shadow-card",
                    p { class: "m-0 text-xs font-medium text-fg-muted", "Dictations today" }
                    if let Some((dictations, _)) = totals_value {
                        p { class: "mb-0 mt-2 text-2xl font-semibold", "{dictations}" }
                    } else {
                        p { class: "mb-0 mt-2 text-2xl font-semibold text-fg-subtle", "—" }
                    }
                }
                div { class: "rounded-lg border border-line bg-surface p-4 shadow-card",
                    p { class: "m-0 text-xs font-medium text-fg-muted", "Words today" }
                    if let Some((_, words)) = totals_value {
                        p { class: "mb-0 mt-2 text-2xl font-semibold", "{words}" }
                    } else {
                        p { class: "mb-0 mt-2 text-2xl font-semibold text-fg-subtle", "—" }
                    }
                }
                div { class: "flex items-center justify-between gap-3 rounded-lg border border-line bg-surface p-4 shadow-card",
                    div {
                        p { class: "m-0 text-xs font-medium text-fg-muted", "Deepgram" }
                        if configured() {
                            p { class: "mb-0 mt-2 text-sm font-semibold text-success", "Connected" }
                            p { class: "mb-0 mt-1 text-xs text-fg-muted",
                                if let Some(amount) = balance() { "{amount}" }
                                else if balance_error().is_some() { "Balance unavailable" }
                                else { "Loading balance…" }
                            }
                        } else {
                            p { class: "mb-0 mt-2 text-sm font-semibold", "API key required" }
                        }
                    }
                    if !configured() {
                        button {
                            class: "rounded-md border border-line px-3 py-2 text-xs font-medium hover:bg-control-hover",
                            r#type: "button",
                            onclick: move |_| setup_bus.open_main(Some(Section::Transcription)),
                            "Set up"
                        }
                    }
                }
            }

            if history_error().is_some() {
                div { class: "flex items-center justify-between gap-3 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger", role: "alert",
                    span { "Couldn't load history" }
                    button {
                        class: "shrink-0 underline",
                        r#type: "button",
                        onclick: move |_| {
                            let state = Arc::clone(&retry_state);
                            spawn(async move {
                                apply_dashboard(load_dashboard(state).await, totals, recent, configured, history_error);
                            });
                        },
                        "Retry"
                    }
                }
            }
            if let Some(message) = copy_error() {
                div { class: "rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger", role: "alert", "{message}" }
            }

            section { class: "flex flex-col gap-3",
                div { class: "flex items-center justify-between",
                    h2 { class: "m-0 text-base font-semibold", "Recent" }
                    button {
                        class: "text-sm font-medium text-accent hover:underline",
                        r#type: "button",
                        onclick: move |_| history_bus.open_main(Some(Section::History)),
                        "View all"
                    }
                }
                if recent_entries.is_empty() && history_error().is_none() {
                    div { class: "rounded-lg border border-line bg-surface",
                        EmptyState { icon: IconName::HistoryEmpty, title: "No dictations yet".to_string() }
                    }
                } else if !recent_entries.is_empty() {
                    div { class: "overflow-hidden rounded-lg border border-line bg-surface shadow-card",
                        for entry in recent_entries {
                            {
                                let text = entry.formatted_text.clone().or(entry.asr_text.clone()).unwrap_or_default();
                                let copy_text_value = text.clone();
                                let app = if entry.app_name.is_empty() { "Unknown app".to_string() } else { entry.app_name.clone() };
                                let time = Local.timestamp_opt(entry.timestamp as i64, 0).single().map(|time| time.format("%-I:%M %p").to_string()).unwrap_or_default();
                                let duration = format!("{:.1}s", entry.duration_secs);
                                rsx! {
                                    div { class: "group flex items-start gap-4 border-b border-line px-4 py-3 last:border-b-0",
                                        div { class: "min-w-0 flex-1",
                                            p { class: "m-0 line-clamp-2 whitespace-pre-wrap text-[13px]", "{text}" }
                                            p { class: "mb-0 mt-2 text-xs text-fg-muted",
                                                "{time} · {app} · {duration} · {entry.num_words} words"
                                            }
                                        }
                                        button {
                                            class: "rounded-md p-2 text-fg-muted opacity-0 hover:bg-control-hover hover:text-fg group-hover:opacity-100 focus:opacity-100",
                                            r#type: "button",
                                            title: "Copy",
                                            onclick: move |_| {
                                                if let Err(error) = copy_text(&copy_text_value) {
                                                    copy_error.set(Some(format!("Could not copy to the clipboard: {error}")));
                                                } else {
                                                    copy_error.set(None);
                                                }
                                            },
                                            Icon { name: IconName::Copy, size: 16 }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
