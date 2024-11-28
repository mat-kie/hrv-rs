//! View Manager
//!
//! This module provides the view management layer for the HRV analysis tool.
//! It includes structures and methods for switching between different views and managing their lifecycle.

use crate::{
    core::{
        events::{AppEvent, ViewState},
        view_trait::ViewApi,
    },
    model::bluetooth::AdapterHandle,
    view::{bluetooth::BluetoothView, hrv_analysis::HrvView},
};
use eframe::App;
use log::{error, info, warn};
use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use tokio::{
    sync::broadcast::{error::RecvError, Sender},
};

/// `ViewManager<AHT>` structure.
///
/// Manages UI views and facilitates transitions based on application state.
/// This structure allows switching between multiple views and handles their lifecycle.
pub struct ViewManager<AHT: AdapterHandle + Send> {
    /// Shared reference to the currently active view.
    current_view: Arc<Mutex<Option<Box<dyn ViewApi>>>>,
    /// Task handle for the view state update listener.
    /// Event channel for sending application events from views.
    event_ch: Sender<AppEvent>,
    /// Marker to track the generic adapter handle type.
    _marker: PhantomData<AHT>,
}

impl<AHT: AdapterHandle + Send + 'static> ViewManager<AHT> {
    /// Creates a new `ViewManager` and starts a task to listen for view state changes.
    ///
    /// # Arguments
    /// * `rx_channel` - A receiver for `ViewState` updates.
    /// * `event_ch` - A channel for sending `AppEvent` messages from views.
    ///
    /// # Returns
    /// A new instance of `ViewManager`.
    pub fn new(
        mut rx_channel: tokio::sync::broadcast::Receiver<ViewState<AHT>>,
        event_ch: Sender<AppEvent>,
    ) -> Self {
        let current_view = Arc::new(Mutex::new(None));
        let view_clone = Arc::clone(&current_view);

        // Spawn a task to handle view state transitions.
        tokio::spawn(async move {
            loop {
                match rx_channel.recv().await {
                    Ok(state) => {
                        info!("Received ViewState update");
                        ViewManager::update_view(view_clone.clone(), state);
                    }
                    Err(RecvError::Closed) => {
                        warn!("View state receiver channel closed.");
                        break;
                    }
                    Err(RecvError::Lagged(count)) => {
                        warn!("View state receiver lagged by {} messages.", count);
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        });

        Self {
            current_view,
            event_ch,
            _marker: Default::default(),
        }
    }

    /// Updates the current view based on the given `ViewState`.
    ///
    /// # Arguments
    /// * `current_view` - A shared reference to the current view.
    /// * `state` - The new `ViewState` to transition to.
    fn update_view(current_view: Arc<Mutex<Option<Box<dyn ViewApi>>>>, state: ViewState<AHT>) {
        let mut view_guard = current_view.lock().unwrap();
        *view_guard = match state {
            ViewState::BluetoothSelectorView(model) => {
                info!("Switching to BluetoothSelectorView");
                Some(Box::new(BluetoothView::new(model)))
            }
            ViewState::AcquisitionView(model) => {
                info!("Switching to AcquisitionView");
                Some(Box::new(HrvView::new(model)))
            }
        };
    }
}

impl<AHT: AdapterHandle + Send> App for ViewManager<AHT> {
    /// Updates the current view and processes events.
    ///
    /// # Arguments
    /// * `ctx` - The `egui::Context` for rendering.
    /// * `_frame` - The `eframe::Frame` for controlling the application window.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match self.current_view.lock() {
            Ok(view_guard) => {
                if let Some(view) = &*view_guard {
                    if let Some(event) = view.render(ctx) {
                        if let Err(err) = self.event_ch.send(event) {
                            error!("Failed to send view event: {}", err);
                        }
                    }
                } else {
                    warn!("No active view to render.");
                }
            }
            Err(err) => {
                error!("Failed to acquire lock on current view: {}", err);
            }
        }
    }
}
