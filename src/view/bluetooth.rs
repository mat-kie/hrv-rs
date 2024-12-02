//! Bluetooth View
//!
//! This module provides the view layer for managing Bluetooth interactions in the HRV analysis tool.
//! It includes structures and methods for rendering the Bluetooth device selector and interaction UI.

use eframe::egui;
use egui::Color32;
use log::error;
use tokio::sync::mpsc::Sender;
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
    event_ch: Sender<AppEvent>,
    /// A marker to track the generic type for the adapter handle.
    _marker: PhantomData<AHT>,
}

impl<AHT: AdapterHandle + 'static> BluetoothView<AHT> {
    /// Creates a new `BluetoothView` instance.
    ///
    /// # Arguments
    /// * `model` - Shared access to the `BluetoothModel`.
    pub fn new(model: Arc<tokio::sync::Mutex<dyn BluetoothModelApi<AHT>>>, event_ch:Sender<AppEvent>) -> Self {
        Self {
            model,
            event_ch,
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
    fn render_adapters(&self, ui: &mut egui::Ui) {
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
                self.event(AppEvent::Bluetooth(BluetoothEvent::AdapterSelected(uuid)));
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
    fn render_devices(&self, ui: &mut egui::Ui) {
        let model = self.model.blocking_lock();
        ui.heading("Discovered Devices:");
        
            for (addr, device) in model.get_devices() {
                ui.vertical(|ui| {

                let btn = egui::Button::new(device);
                
                if ui.add_sized([ui.available_width(), 20.0], btn).clicked() {
                    self.event(AppEvent::Bluetooth(BluetoothEvent::StartListening(*addr)));
                }
                
            });
            }
              
       
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

    fn event(&self, event: AppEvent) {
        if let Err(e)= self.event_ch.try_send(event){
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
    fn render(&self, ctx: &egui::Context) -> Result<(), String> {
        egui::CentralPanel::default()
            .show(ctx, |ui| {
                ui.heading("Please select Bluetooth device");
                egui::ScrollArea::vertical()
                    .show(ui, |ui| {
                        self.render_adapters(ui);
                        self.render_scanning_status(ui);
                        ui.separator();
                        self.render_devices(ui);
                        
                    })
            });
        Ok(())
    }
}
