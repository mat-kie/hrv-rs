//! Bluetooth View
//!
//! This module provides the view layer for managing Bluetooth interactions in the HRV analysis tool.
//! It includes structures and methods for rendering the Bluetooth device selector and interaction UI.

use eframe::egui;
use egui::Color32;
use std::{marker::PhantomData, sync::Arc};

use crate::{
    core::{events::AppEvent, events::BluetoothEvent, view_trait::ViewApi},
    model::bluetooth::{AdapterHandle, BluetoothModelApi},
};

/// The `BluetoothView` renders a UI for selecting Bluetooth adapters and devices.
///
/// Represents the view for managing Bluetooth interactions, such as device selection and connection.
pub struct BluetoothView<AHT: AdapterHandle> {
    /// The shared Bluetooth model that provides adapter and device information.
    model: Arc<tokio::sync::Mutex<dyn BluetoothModelApi<AHT>>>,
    /// A marker to track the generic type for the adapter handle.
    _marker: PhantomData<AHT>,
}

impl<AHT: AdapterHandle> BluetoothView<AHT> {
    /// Creates a new `BluetoothView` instance.
    ///
    /// # Arguments
    /// * `model` - Shared access to the `BluetoothModel`.
    pub fn new(model: Arc<tokio::sync::Mutex<dyn BluetoothModelApi<AHT>>>) -> Self {
        Self {
            model,
            _marker: Default::default(),
        }
    }

    /// Renders the list of Bluetooth adapters and returns the selected adapter index if clicked.
    ///
    /// # Arguments
    /// * `ui` - The egui UI element for rendering.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by adapter selection.
    fn render_adapters(&self, ui: &mut egui::Ui) -> Option<AppEvent> {
        let model = self.model.blocking_lock();
        let selected = model.get_selected_adapter();
        ui.label("Select a Bluetooth Adapter:");
        for (adapter_name, uuid) in model.get_adapter_names() {
            let mut btn = egui::Button::new(adapter_name);
            if let Some(sel) = selected {
                if &uuid == sel.uuid() {
                    btn = btn.fill(Color32::DARK_BLUE);
                }
            }

            if ui.add(btn).clicked() {
                return Some(AppEvent::Bluetooth(BluetoothEvent::AdapterSelected(uuid)));
            }
        }
        None
    }

    /// Renders the list of discovered Bluetooth devices and returns the selected device address if clicked.
    ///
    /// # Arguments
    /// * `ui` - The egui UI element for rendering.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by device selection.
    fn render_devices(&self, ui: &mut egui::Ui) -> Option<AppEvent> {
        let model = self.model.blocking_lock();
        ui.heading("Discovered Devices:");
        
            for (addr, device) in model.get_devices() {
                if let Some(evt) = ui.vertical(|ui| {

                let btn = egui::Button::new(device);
                
                if ui.add_sized([ui.available_width(), 20.0], btn).clicked() {
                    return Some(AppEvent::Bluetooth(BluetoothEvent::StartListening(*addr)));
                }
                None
            }).inner
            {
                return Some(evt)
            }
            }
            None
        
       
       
    }

    /// Renders the scanning status in the UI.
    ///
    /// # Arguments
    /// * `ui` - The egui UI element for rendering.
    fn render_scanning_status(&self, ui: &mut egui::Ui) {
        let model = self.model.blocking_lock();
        if model.is_scanning() {
            ui.label("Scanning...");
        }
    }
}

impl<AHT: AdapterHandle + Send + 'static> ViewApi for BluetoothView<AHT> {
    /// Renders the complete Bluetooth view UI.
    ///
    /// # Arguments
    /// * `ctx` - The egui context for rendering the UI.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by user interactions.
    fn render(&self, ctx: &egui::Context) -> Option<AppEvent> {
        egui::CentralPanel::default()
            .show(ctx, |ui| {
                ui.heading("Please select Bluetooth device");
                egui::ScrollArea::vertical()
                    .show(ui, |ui| {
                        let selected_adapter = self.render_adapters(ui);
                        if selected_adapter.is_some() {
                            return selected_adapter;
                        }
                        self.render_scanning_status(ui);
                        ui.separator();
                        let selected_peripheral = self.render_devices(ui);
                        if selected_peripheral.is_some() {
                            return selected_peripheral;
                        }
                        None
                    })
                    .inner
            })
            .inner
    }
}
