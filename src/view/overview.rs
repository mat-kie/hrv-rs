//! Storage View
//!
//! This module provides the view layer for managing stored acquisitions in the HRV analysis tool.
//! It includes structures and methods for rendering the UI for selecting and interacting with stored acquisitions.

use time::macros::format_description;

use crate::{
    api::{
        model::{MeasurementModelApi, ModelHandle, StorageModelApi},
        view::ViewApi,
    },
    core::events::{AppEvent, StateChangeEvent, StorageEvent},
};

use super::acquisition::{
    render_filter_params, render_poincare_plot, render_stats, render_time_series,
};

/// The `StorageView` renders a UI for managing stored acquisitions.
///
/// Represents the view for managing stored acquisitions, such as selecting, viewing, and interacting with them.
pub struct StorageView {
    /// The shared storage model that provides acquisition information.
    model: ModelHandle<dyn StorageModelApi>,
    /// The currently selected acquisition.
    selected: Option<ModelHandle<dyn MeasurementModelApi>>,
}

impl StorageView {
    /// Creates a new `StorageView`.
    ///
    /// # Arguments
    /// * `model` - The storage model handle.
    ///
    /// # Returns
    /// A new instance of `StorageView`.
    pub fn new(
        model: ModelHandle<dyn StorageModelApi>,
        selected: Option<ModelHandle<dyn MeasurementModelApi>>,
    ) -> Self {
        Self { model, selected }
    }
}

impl ViewApi for StorageView {
    /// Renders the current view.
    ///
    /// # Arguments
    /// * `publish` - Function to publish `UiInputEvent`s.
    /// * `ctx` - Egui context for rendering.
    ///
    /// # Returns
    /// `Result<(), String>` indicating success or an error message.
    fn render<F: Fn(AppEvent) + ?Sized>(
        &mut self,
        publish: &F,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let model = self.model.blocking_read();
        // Render the top menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                // "File" Menu
                ui.menu_button("File", |ui| {
                    if ui.button("Open").clicked() {
                        if let Some(file) = rfd::FileDialog::new().pick_file() {
                            publish(AppEvent::Storage(StorageEvent::LoadFromFile(file)))
                        }
                        ui.close_menu(); // Close the menu after selection
                    }
                    if ui.button("Save").clicked() {
                        if let Some(file) = rfd::FileDialog::new().save_file() {
                            publish(AppEvent::Storage(StorageEvent::StoreToFile(file)))
                        }
                        ui.close_menu();
                    }
                    if ui.button("New").clicked() {
                        publish(AppEvent::Storage(StorageEvent::Clear));

                        ui.close_menu();
                    }
                });
            });
        });

        // Render the left side panel with past measurements
        let fd = format_description!("[year]-[month]-[day] [hour]:[minute]");
        egui::SidePanel::left("left_overview").show(ctx, |ui| {
            ui.add_space(10.0);
            ui.heading("Past Measurements:");
            for (idx, acq) in model.get_acquisitions().iter().enumerate() {
                let btn: egui::Button<'_> = egui::Button::new(
                    acq.blocking_read()
                        .get_start_time()
                        .format(fd)
                        .unwrap()
                        .to_string(),
                );
                if ui.add_sized([ui.available_width(), 20.0], btn).clicked() {
                    publish(AppEvent::AppState(StateChangeEvent::SelectMeasurement(idx)));
                }
            }
            ui.separator();
            if ui.button("New Acquisition").clicked() {
                publish(AppEvent::AppState(StateChangeEvent::ToRecordingState));
            }
        });

        // Render the right side panel with selected acquisition details
        if let Some(selected) = &self.selected {
            let lck = selected.blocking_read();
            egui::SidePanel::right("right:overview").show(ctx, |ui| {
                let model = &*lck;
                let hr = if let Some(hr) = model.get_hr() {
                    hr
                } else {
                    0.0
                };
                render_stats(ui, model, hr);
                ui.separator();
                render_filter_params(ui, &publish, model);
            });

            // Render the bottom panel with time series data
            egui::TopBottomPanel::bottom("time series panel")
                .min_height(100.0)
                .resizable(true)
                .show(ctx, |ui| {
                    let model = &*lck;
                    render_time_series(ui, model);
                });

            // Render the central panel with Poincaré plot
            egui::CentralPanel::default().show(ctx, |ui| {
                let model = &*lck;
                render_poincare_plot(ui, model);
            });
        }
        Ok(())
    }
}
