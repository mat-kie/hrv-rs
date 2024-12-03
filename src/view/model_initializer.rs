//! Bluetooth View
//!
//! This module provides the view layer for managing Bluetooth interactions in the HRV analysis tool.
//! It includes structures and methods for rendering the Bluetooth device selector and interaction UI.

use eframe::egui;
use log::{error, info};
use tokio::sync::mpsc::Sender;

use crate::core::{events::AppEvent, view_trait::ViewApi};

/// The `BluetoothView` renders a UI for selecting Bluetooth adapters and devices.
///
/// Represents the view for managing Bluetooth interactions, such as device selection and connection.
pub struct ModelInitView {
    event_ch: Sender<AppEvent>,
}

impl ModelInitView {
    /// Creates a new `BluetoothView` instance.
    ///
    /// # Arguments
    /// * `model` - Shared access to the `BluetoothModel`.
    pub fn new(event_ch: Sender<AppEvent>) -> Self {
        Self { event_ch }
    }
}

impl ViewApi for ModelInitView {
    fn event(&self, event: AppEvent) {
        if let Err(e) = self.event_ch.try_send(event) {
            error!("Failed to send AppEvent: {}", e);
        }
    }
    /// Renders the complete Bluetooth view UI.
    ///
    /// # Arguments
    /// * `ctx` - The egui context for rendering the UI.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by user interactions.
    fn render(&mut self, ctx: &egui::Context) -> Result<(), String> {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("new file").clicked() {
                    // new Model
                    if let Some(file) = rfd::FileDialog::new().save_file() {
                        info!("new file clicked {:?}", file);
                        self.event(AppEvent::NewModel);
                        self.event(AppEvent::StoreModel(file));
                    }
                    // open rfd in new file mode
                    // serialize and store to queried location
                    // store path to egui Storage
                } else if ui.button("load file").clicked() {
                    // load model from file
                    if let Some(file) = rfd::FileDialog::new().pick_file() {
                        info!("load file clicked {:?}", file);
                        self.event(AppEvent::LoadModel(file));
                    }

                    // set egui storage to file path when loaded successfull
                }
            })
            .inner
        });
        Ok(())
    }
}
