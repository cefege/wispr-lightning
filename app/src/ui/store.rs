use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use wl_core::settings::Settings;
use wl_shell::state::AppState;

use crate::bus::Bus;

#[derive(Clone, Copy)]
pub struct SettingsStore {
    pub value: Signal<Settings>,
    pub save_error: Signal<Option<String>>,
    pending: Signal<u64>,
    last_sent: Signal<Settings>,
    state: Signal<Arc<AppState>>,
}

impl SettingsStore {
    fn new(initial: Settings, state: Arc<AppState>) -> Self {
        Self {
            value: Signal::new(initial.clone()),
            save_error: Signal::new(None),
            pending: Signal::new(0),
            last_sent: Signal::new(initial),
            state: Signal::new(state),
        }
    }

    pub fn update(&mut self, f: impl FnOnce(&mut Settings)) {
        let mut next = self.value.read().clone();
        f(&mut next);
        self.value.set(next);
        let generation = *self.pending.read() + 1;
        self.pending.set(generation);

        let mut store = *self;
        let state = store.state.read().clone();
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if *store.pending.read() != generation {
                return;
            }
            let next = store.value.read().clone();
            let state_for_save = Arc::clone(&state);
            let result = tokio::task::spawn_blocking(move || state_for_save.save_settings(next))
                .await
                .map_err(|error| format!("could not save settings: {error}"))
                .and_then(|result| result);
            match result {
                Ok(saved) => {
                    store.last_sent.set(saved.clone());
                    if *store.pending.read() == generation {
                        store.value.set(saved);
                        store.save_error.set(None);
                    }
                }
                Err(message) if *store.pending.read() == generation => {
                    store.save_error.set(Some(message));
                }
                Err(_) => {}
            }
        });
    }

    pub async fn flush(&mut self) -> Result<(), String> {
        let generation = *self.pending.read() + 1;
        self.pending.set(generation);
        let next = self.value.read().clone();
        let state = self.state.read().clone();
        let result = tokio::task::spawn_blocking(move || state.save_settings(next))
            .await
            .map_err(|error| format!("could not save settings: {error}"))
            .and_then(|result| result);
        match result {
            Ok(saved) => {
                self.value.set(saved.clone());
                self.last_sent.set(saved);
                self.save_error.set(None);
                Ok(())
            }
            Err(message) => {
                self.save_error.set(Some(message.clone()));
                Err(message)
            }
        }
    }

    pub async fn complete_onboarding(&mut self) -> Result<(), String> {
        self.flush().await?;
        let state = self.state.read().clone();
        let completed = tokio::task::spawn_blocking(move || {
            wl_shell::ops::onboarding_complete(&state)
        })
        .await
        .map_err(|error| format!("could not complete onboarding: {error}"))??;
        self.value.set(completed.clone());
        self.last_sent.set(completed);
        self.save_error.set(None);
        Ok(())
    }

    pub async fn restart_onboarding(&mut self) -> Result<(), String> {
        self.update(|settings| settings.did_complete_onboarding = false);
        self.flush().await
    }
}

/// Install one store per main-window document and follow changes from other
/// app surfaces without overwriting a local debounce.
pub fn provide() {
    let state = use_context::<Arc<AppState>>();
    let bus = use_context::<Arc<Bus>>();
    let store = use_context_provider(|| SettingsStore::new(state.settings(), state));

    use_future(move || {
        let mut settings = bus.settings.subscribe();
        let mut errors = bus.settings_error.subscribe();
        let mut store = store;
        async move {
            loop {
                tokio::select! {
                    changed = settings.changed() => {
                        if changed.is_err() { return; }
                        let incoming = settings.borrow_and_update().clone();
                        let last_sent = store.last_sent.read().clone();
                        if store.value.read().clone() == last_sent && incoming != last_sent {
                            store.value.set(incoming.clone());
                            store.last_sent.set(incoming);
                            store.save_error.set(None);
                        }
                    }
                    error = errors.recv() => {
                        match error {
                            Ok(message) => store.save_error.set(Some(message)),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        }
                    }
                }
            }
        }
    });
}
