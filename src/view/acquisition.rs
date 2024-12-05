//! HRV Analysis View
//!
//! This module provides the view layer for visualizing HRV (Heart Rate Variability) analysis results.
//! It includes structures and methods for rendering statistical data, charts, and user interface components.

use crate::{
    core::{events::UiInputEvent, view_trait::ViewApi},
    model::{acquisition::AcquisitionModelApi, bluetooth::BluetoothModelApi, storage::ModelHandle},
};
use eframe::egui;
use egui::Color32;
use egui_plot::{Legend, Plot, Points};
use log::info;
use std::ops::RangeInclusive;
use time::Duration;

pub fn render_stats(ui: &mut egui::Ui, model: &dyn AcquisitionModelApi, hr: f64) {
    ui.heading("Statistics");
    egui::Grid::new("stats grid").num_columns(2).show(ui, |ui| {
        let desc = egui::Label::new("Heartrate: ");
        ui.add(desc);
        let val = egui::Label::new(format!("{:.2} BPM", hr));
        ui.add(val);
        ui.end_row();

        let desc = egui::Label::new("Elapsed time: ");
        ui.add(desc);
        let val = egui::Label::new(format!("{} s", model.get_elapsed_time().whole_seconds()));
        ui.add(val);
        ui.end_row();

        if let Some(stats) = model.get_hrv_stats() {
            let desc = egui::Label::new("RMSSD [ms]");
            ui.add(desc);
            let val = egui::Label::new(format!("{:.2} ms", stats.rmssd));
            ui.add(val);
            ui.end_row();
            let desc = egui::Label::new("SDRR [ms]");
            ui.add(desc);
            let val = egui::Label::new(format!("{:.2} ms", stats.sdrr));
            ui.add(val);
            ui.end_row();
            let desc = egui::Label::new("SD1 [ms]");
            ui.add(desc);
            let val = egui::Label::new(format!("{:.2} ms", stats.sd1));
            ui.add(val);
            ui.end_row();
            let desc = egui::Label::new("SD2 [ms]");
            ui.add(desc);
            let val = egui::Label::new(format!("{:.2} ms", stats.sd2));
            ui.add(val);
            ui.end_row();
        }
    });
}

pub fn render_time_series(ui: &mut egui::Ui, model: &dyn AcquisitionModelApi) {
    let plot: Plot<'_> = Plot::new("Time series").legend(Legend::default());

    plot.show(ui, |plot_ui| {
        plot_ui.line(
            egui_plot::Line::new(model.get_session_data().rmssd_ts.clone())
                .name("RMSSD [ms]")
                .color(Color32::RED),
        );
        plot_ui.line(
            egui_plot::Line::new(model.get_session_data().sd1_ts.clone())
                .name("SD1 [ms]")
                .color(Color32::BLUE),
        );
        plot_ui.line(
            egui_plot::Line::new(model.get_session_data().sd2_ts.clone())
                .name("SD2 [ms]")
                .color(Color32::YELLOW),
        );
        plot_ui.line(
            egui_plot::Line::new(model.get_session_data().hr_ts.clone())
                .name("HR [1/min]")
                .color(Color32::GREEN),
        );
    });
}

pub fn render_poincare_plot(ui: &mut egui::Ui, model: &dyn AcquisitionModelApi) {
    let plot = Plot::new("Poincare Plot")
        .legend(Legend::default())
        .data_aspect(1.0);

    plot.show(ui, |plot_ui| {
        plot_ui.points(
            Points::new(model.get_poincare_points())
                .name("R-R Intervals")
                .shape(egui_plot::MarkerShape::Diamond)
                .color(Color32::RED)
                .radius(5.0),
        );
    });
}

pub fn render_bluetooth<F: Fn(UiInputEvent) + ?Sized>(
    ui: &mut egui::Ui,
    publish: &F,
    model: &dyn BluetoothModelApi,
) {
    ui.heading("Bluetooth settings:");
    ui.add_enabled_ui(model.get_selected_adapter().is_none(), |ui| {
        let current = model.get_selected_adapter();
        egui::ComboBox::from_label("Adapter")
            .selected_text(
                model
                    .get_selected_adapter()
                    .as_ref()
                    .map_or(Default::default(), |a| a.get_name().to_owned()),
            )
            .show_ui(ui, |ui| {
                for adapter in model.get_adapters() {
                    if ui
                        .selectable_label(
                            current
                                .as_ref()
                                .map_or(false, |a| a.get_uuid() == adapter.get_uuid()),
                            adapter.get_name(),
                        )
                        .clicked()
                    {
                        publish(UiInputEvent::SelectAdapter(adapter.clone()));
                    }
                }
            });
    });

    ui.add_enabled_ui(model.is_listening_to().is_none(), |ui| {
        let current = model.get_selected_device();
        egui::ComboBox::from_label("Device")
            .selected_text(
                model
                    .get_selected_device()
                    .as_ref()
                    .map_or(Default::default(), |a| a.name.to_owned()),
            )
            .show_ui(ui, |ui| {
                for device in model.get_devices() {
                    if ui
                        .selectable_label(
                            current
                                .as_ref()
                                .map_or(false, |a| a.address == device.address),
                            device.name.clone(),
                        )
                        .clicked()
                    {
                        publish(UiInputEvent::SelectPeripheral(device.clone()));
                    }
                }
            });
    });
}

pub fn render_filter_params<F: Fn(UiInputEvent)>(
    ui: &mut egui::Ui,
    publish: &F,
    model: &dyn AcquisitionModelApi,
) {
    ui.heading("Filter parameters:");
    egui::Grid::new("a grid").num_columns(2).show(ui, |ui| {
        let mut seconds = model
            .get_stats_window()
            .unwrap_or(Duration::minutes(5))
            .as_seconds_f64();
        let desc = egui::Label::new("time window [s]");
        ui.add(desc);
        let slider = egui::Slider::new(&mut seconds, RangeInclusive::new(0.0, 600.0));
        if ui.add(slider).changed() {
            if let Some(new_duration) = Duration::checked_seconds_f64(seconds) {
                info!("changed value to: {}", seconds);
                publish(UiInputEvent::TimeWindowChanged(new_duration));
            }
        }
        ui.end_row();
        let mut outlier_value = model.get_outlier_filter_value();
        let desc = egui::Label::new("outlier filter");
        ui.add(desc);
        let slider = egui::Slider::new(&mut outlier_value, RangeInclusive::new(0.1, 400.0));
        if ui.add(slider).changed() {
            info!("changed value to: {}", outlier_value);
            publish(UiInputEvent::OutlierFilterChanged(outlier_value));
        }
        ui.end_row();
    });
}
/// `HrvView` structure.
///
/// Represents the view for visualizing HRV analysis results, including statistics and charts.
pub struct AcquisitionView {
    /// Shared access to the runtime HRV data model.
    model: ModelHandle<dyn AcquisitionModelApi>,
    bt_model: ModelHandle<dyn BluetoothModelApi>,
}

impl AcquisitionView {
    /// Creates a new `HrvView` instance.
    ///
    /// # Arguments
    /// * `model` - Shared access to the runtime HRV data.
    ///
    /// # Returns
    /// A new `HrvView` instance.
    pub fn new(
        model: ModelHandle<dyn AcquisitionModelApi>,
        bt_model: ModelHandle<dyn BluetoothModelApi>,
    ) -> Self {
        Self { model, bt_model }
    }

    fn render_acq<F: Fn(UiInputEvent)>(&self, ui: &mut egui::Ui, publish: &F) {
        ui.heading("Acquisition");
        ui.horizontal(|ui| {
            if ui.button("start").clicked() {
                publish(UiInputEvent::AcquisitionStartReq);
            }
            if ui.button("stop").clicked() {
                publish(UiInputEvent::AcquisitionStopReq);
            }
            if ui.button("discard").clicked() {
                publish(UiInputEvent::AcquisitionStopReq);
                publish(UiInputEvent::DiscardAcquisition);
            }
            if ui.button("Save").clicked() {
                publish(UiInputEvent::StoreAcquisition);
            }
        });
    }
}

impl ViewApi for AcquisitionView {
    /// Renders the complete HRV analysis view.
    ///
    /// Displays both the HRV statistics panel and the Poincare plot.
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
        // Extract HRV statistics and Poincare plot points from the model.

        // Render the left panel with HRV statistics.
        let model = self.model.blocking_read();
        let bt_model = self.bt_model.blocking_read();
        egui::SidePanel::left("left_sidebar").show(ctx, |ui| {
            render_bluetooth(ui, publish, &*bt_model);
            ui.separator();

            self.render_acq(ui, &publish);
            ui.separator();
            render_filter_params(ui, &publish, &*model);
            let msg = model.get_last_msg();
            if let Some(msg) = msg {
                ui.separator();
                render_stats(ui, &*model, msg.get_hr());
            }
        });

        egui::TopBottomPanel::bottom("time series panel").min_height(100.0).resizable(true).show(ctx, |ui|{
            render_time_series(ui, &*model);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            render_poincare_plot(ui, &*model);
        });

        Ok(()) // no errors
    }
}
