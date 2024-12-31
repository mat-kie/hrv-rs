//! HRV Analysis View
//!
//! This module provides the view layer for visualizing HRV (Heart Rate Variability) analysis results.
//! It includes structures and methods for rendering statistical data, charts, and user interface components.

use eframe::egui;
use egui::Color32;
use egui_plot::{Legend, Plot, Points};
use std::ops::RangeInclusive;

use crate::{
    api::{
        controller::OutlierFilter,
        model::{BluetoothModelApi, MeasurementModelApi, ModelHandle},
        view::ViewApi,
    },
    core::events::{AppEvent, BluetoothEvent, MeasurementEvent, RecordingEvent, StateChangeEvent},
};

fn render_labelled_data(ui: &mut egui::Ui, label: &str, data: Option<String>) {
    if let Some(data) = data {
        let desc = egui::Label::new(label);
        ui.add(desc);
        let val = egui::Label::new(data);
        ui.add(val);
    }
}

pub fn render_stats(ui: &mut egui::Ui, model: &dyn MeasurementModelApi, hr: f64) {
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
        render_labelled_data(
            ui,
            "RMSSD",
            model.get_rmssd().map(|val| format!("{:.2} ms", val)),
        );
        ui.end_row();
        render_labelled_data(
            ui,
            "SDRR",
            model.get_sdrr().map(|val| format!("{:.2} ms", val)),
        );
        ui.end_row();
        render_labelled_data(
            ui,
            "SD1",
            model.get_sd1().map(|val| format!("{:.2} ms", val)),
        );
        ui.end_row();
        render_labelled_data(
            ui,
            "SD2",
            model.get_sd2().map(|val| format!("{:.2} ms", val)),
        );
        ui.end_row();
        render_labelled_data(
            ui,
            "DFA 1 alpha",
            model.get_dfa1a().map(|val| format!("{:.2} ms", val)),
        );
        ui.end_row();
    });
}

pub fn render_time_series(ui: &mut egui::Ui, model: &dyn MeasurementModelApi) {
    let plot: Plot<'_> = Plot::new("Time series").legend(Legend::default());

    plot.show(ui, |plot_ui| {
        plot_ui.line(
            egui_plot::Line::new(model.get_rmssd_ts())
                .name("RMSSD [ms]")
                .color(Color32::RED),
        );
        plot_ui.line(
            egui_plot::Line::new(model.get_sdrr_ts())
                .name("SDRR [ms]")
                .color(Color32::DARK_GREEN),
        );
        plot_ui.line(
            egui_plot::Line::new(model.get_sd1_ts())
                .name("SD1 [ms]")
                .color(Color32::BLUE),
        );
        plot_ui.line(
            egui_plot::Line::new(model.get_sd2_ts())
                .name("SD2 [ms]")
                .color(Color32::YELLOW),
        );
        plot_ui.line(
            egui_plot::Line::new(model.get_hr_ts())
                .name("HR [1/min]")
                .color(Color32::GREEN),
        );

        plot_ui.line(
            egui_plot::Line::new(model.get_dfa1a_ts())
                .name("DFA 1 alpha")
                .color(Color32::KHAKI),
        );
    });
}

pub fn render_poincare_plot(ui: &mut egui::Ui, model: &dyn MeasurementModelApi) {
    let plot = Plot::new("Poincare Plot")
        .legend(Legend::default())
        .data_aspect(1.0);

    plot.show(ui, |plot_ui| {
        if let Ok((inliers, outliers)) = model.get_poincare_points() {
            plot_ui.points(
                Points::new(inliers)
                    .name("R-R")
                    .shape(egui_plot::MarkerShape::Diamond)
                    .color(Color32::RED)
                    .radius(5.0),
            );
            plot_ui.points(
                Points::new(outliers)
                    .name("R-R outliers")
                    .shape(egui_plot::MarkerShape::Diamond)
                    .color(Color32::GRAY)
                    .radius(5.0),
            );
        }
    });
}

pub fn render_bluetooth<F: Fn(AppEvent) + ?Sized>(
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
                        publish(AppEvent::Bluetooth(BluetoothEvent::SelectAdapter(
                            adapter.clone(),
                        )));
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
                let dlock = model.get_devices().blocking_read();
                for device in dlock.iter() {
                    if ui
                        .selectable_label(
                            current
                                .as_ref()
                                .map_or(false, |a| a.address == device.address),
                            device.name.clone(),
                        )
                        .clicked()
                    {
                        publish(AppEvent::Bluetooth(BluetoothEvent::SelectPeripheral(
                            device.clone(),
                        )));
                    }
                }
            });
    });
}

pub fn render_filter_params<F: Fn(AppEvent)>(
    ui: &mut egui::Ui,
    publish: &F,
    model: &dyn MeasurementModelApi,
) {
    ui.heading("Filter parameters:");
    egui::Grid::new("a grid").num_columns(2).show(ui, |ui| {
        let mut samples = model.get_stats_window().unwrap_or(usize::MAX).to_owned();
        let desc = egui::Label::new("window size [# samples]");
        ui.add(desc);
        let slider = egui::Slider::new(&mut samples, RangeInclusive::new(30, 300));
        if ui.add(slider).changed() {
            publish(AppEvent::Measurement(MeasurementEvent::SetStatsWindow(
                samples,
            )));
        }
        ui.end_row();
        let mut outlier_value = model.get_outlier_filter_value();
        let desc = egui::Label::new("outlier filter scale");
        ui.add(desc);
        let slider = egui::Slider::new(&mut outlier_value, RangeInclusive::new(0.5, 10.0));
        if ui.add(slider).changed() {
            publish(AppEvent::Measurement(MeasurementEvent::SetOutlierFilter(
                OutlierFilter::MovingMAD {
                    parameter: outlier_value,
                    _window: 5,
                },
            )));
        }
        ui.end_row();
    });
}
/// `HrvView` structure.
///
/// Represents the view for visualizing HRV analysis results, including statistics and charts.
pub struct AcquisitionView {
    /// Shared access to the runtime HRV data model.
    model: ModelHandle<dyn MeasurementModelApi>,
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
        model: ModelHandle<dyn MeasurementModelApi>,
        bt_model: ModelHandle<dyn BluetoothModelApi>,
    ) -> Self {
        Self { model, bt_model }
    }

    fn render_acq<F: Fn(AppEvent)>(&self, ui: &mut egui::Ui, publish: &F) {
        ui.heading("Acquisition");
        ui.horizontal(|ui| {
            if ui.button("start").clicked() {
                publish(AppEvent::Recording(RecordingEvent::StartRecording));
            }
            if ui.button("stop").clicked() {
                publish(AppEvent::Recording(RecordingEvent::StopRecording));
            }
            if ui.button("discard").clicked() {
                publish(AppEvent::Recording(RecordingEvent::StopRecording));
                publish(AppEvent::AppState(StateChangeEvent::DiscardRecording));
            }
            if ui.button("Save").clicked() {
                publish(AppEvent::Recording(RecordingEvent::StopRecording));
                publish(AppEvent::AppState(StateChangeEvent::StoreRecording));
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
    fn render<F: Fn(AppEvent) + ?Sized>(
        &mut self,
        publish: &F,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let model = self.model.blocking_read();
        let bt_model = self.bt_model.blocking_read();
        // Extract HRV statistics and Poincare plot points from the model.

        // Render the left panel with HRV statistics.
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

        egui::TopBottomPanel::bottom("time series panel")
            .min_height(100.0)
            .resizable(true)
            .show(ctx, |ui| {
                render_time_series(ui, &*model);
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            render_poincare_plot(ui, &*model);
        });

        Ok(()) // no errors
    }
}
