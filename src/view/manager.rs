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
    mem::MaybeUninit,
    sync::{Arc, Mutex},
};
use tokio::sync::{
    broadcast::{error::RecvError, Sender},
    mpsc::{error::TryRecvError, Receiver},
};

/// `ViewManager<AHT>` structure.
///
/// Manages UI views and facilitates transitions based on application state.
/// This structure allows switching between multiple views and handles their lifecycle.
pub struct ViewManager<AHT: AdapterHandle + Send> {
    /// Shared reference to the currently active view.
    current_view: Option<Box<dyn ViewApi>>,
    view_ch: Receiver<Box<dyn ViewApi>>,
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
        view_ch: tokio::sync::mpsc::Receiver<Box<dyn ViewApi>>,
        event_ch: Sender<AppEvent>,
    ) -> Self {
        Self {
            current_view: None,
            view_ch,
            event_ch,
            _marker: Default::default(),
        }
    }
}

impl<AHT: AdapterHandle + Send> App for ViewManager<AHT> {
    /// Updates the current view and processes events.
    ///
    /// # Arguments
    /// * `ctx` - The `egui::Context` for rendering.
    /// * `_frame` - The `eframe::Frame` for controlling the application window.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // TODO: make adjustable
        ctx.set_pixels_per_point(1.5);

        match self.view_ch.try_recv() {
            Ok(new_view) => {
                self.current_view = Some(new_view);
            }
            Err(e) => {
                if e == TryRecvError::Disconnected {
                    warn!("ViewManager: view channel disconnected!");
                }
            }
        }
        if let Some(view) = &self.current_view {
            if let Some(event) = view.render(ctx) {
                if let Err(err) = self.event_ch.send(event) {
                    error!("Failed to send view event: {}", err);
                }
            }
        }
    }
}
