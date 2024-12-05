//! Bluetooth View
//!
//! This module provides the view layer for managing Bluetooth interactions in the HRV analysis tool.
//! It includes structures and methods for rendering the Bluetooth device selector and interaction UI.

use eframe::egui;
use egui::Color32;

use crate::{
    core::{
        events::UiInputEvent,
        view_trait::ViewApi,
    },
    model::{bluetooth::BluetoothModelApi, storage::ModelHandle},
};

/// The `BluetoothView` renders a UI for selecting Bluetooth adapters and devices.
///
/// Represents the view for managing Bluetooth interactions, such as device selection and connection.
pub struct BluetoothView {
    /// The shared Bluetooth model that provides adapter and device information.
    model: ModelHandle<dyn BluetoothModelApi>,
}

impl BluetoothView {
    /// Creates a new `BluetoothView` instance.
    ///
    /// # Arguments
    /// * `model` - Shared access to the `BluetoothModel`.
    pub fn new(model: ModelHandle<dyn BluetoothModelApi>) -> Self {
        Self { model }
    }

    /// Renders the list of Bluetooth adapters and returns the selected adapter index if clicked.
    ///
    /// # Arguments
    /// * `ui` - The egui UI element for rendering.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by adapter selection.
    fn render_adapters<F: Fn(UiInputEvent)>(&self, ui: &mut egui::Ui, publish: &F) {
        let model = self.model.blocking_read();
        let selected = model.get_selected_adapter();
        ui.label("Select a Bluetooth Adapter:");
        for adapter in model.get_adapters() {
            let mut btn = egui::Button::new(adapter.get_name());
            if let Some(sel) = selected {
                if adapter == sel {
                    btn = btn.fill(Color32::DARK_BLUE);
                }
            }
            if ui.add(btn).clicked() {
                publish(UiInputEvent::SelectAdapter(
                    adapter.clone(),
                ));
            }
        }
    }

    /// Renders the list of discovered Bluetooth devices and returns the selected device address if clicked.
    ///
    /// # Arguments
    /// * `ui` - The egui UI element for rendering.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by device selection.
    fn render_devices<F: Fn(UiInputEvent)>(&self, ui: &mut egui::Ui, publish: &F) {
        let model = self.model.blocking_read();
        ui.heading("Discovered Devices:");

        for device in model.get_devices() {
            ui.vertical(|ui| {
                let btn = egui::Button::new(device.name.clone());

                if ui.add_sized([ui.available_width(), 20.0], btn).clicked() {
                    publish(UiInputEvent::SelectPeripheral(
                        device.clone(),
                    ));
                    publish(UiInputEvent::AcquisitionStartReq);
                }
            });
        }
    }

    /// Renders the scanning status in the UI.
    ///
    /// # Arguments
    /// * `ui` - The egui UI element for rendering.
    fn render_scanning_status(&self, ui: &mut egui::Ui) {
        let model = self.model.blocking_read();
        if model.is_scanning() {
            ui.label("Scanning...");
        }
    }
}

impl ViewApi for BluetoothView {
    /// Renders the complete Bluetooth view UI.
    ///
    /// # Arguments
    /// * `ctx` - The egui context for rendering the UI.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by user interactions.
    fn render<F: Fn(UiInputEvent) + ?Sized>(
        &mut self,
        publish: &F,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Please select Bluetooth device");
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.render_adapters(ui, &publish);
                self.render_scanning_status(ui);
                ui.separator();
                self.render_devices(ui,&publish);
            })
        });
        Ok(())
    }
}
