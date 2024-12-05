//! Application Controller
//!
//! This module defines the main controller responsible for orchestrating the application.
//! It initializes and manages other controllers and coordinates the overall application flow.

use super::acquisition::DataAcquisitionApi;
use crate::{
    controller::bluetooth::BluetoothApi,
    core::events::{AppEvent, UiInputEvent},
    model::{acquisition::AcquisitionModel, storage::StorageModel},
    view::manager::{ViewManager, ViewState},
};

use anyhow::Result;
use log::{error, trace};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{broadcast::Sender, RwLock};

/// Main application controller.
///
/// This structure manages the lifecycle of other controllers and handles application-level events.
pub struct AppController<ACT: DataAcquisitionApi + Send + 'static, BTCT: BluetoothApi + 'static> {
    view_tx: Sender<ViewState>,
    event_bus: Sender<AppEvent>,
    ble_controller: BTCT,
    acq_controller: ACT,
    storage: Arc<RwLock<StorageModel<AcquisitionModel>>>,
}

impl<ACT: DataAcquisitionApi + Send + 'static, BTCT: BluetoothApi + 'static>
    AppController<ACT, BTCT>
{
    /// Creates a new `AppController`.
    ///
    /// # Arguments
    /// - `ble_controller`: The Bluetooth controller.
    /// - `acq_controller`: The acquisition controller.
    /// - `storage`: The storage model.
    /// - `event_bus`: The event bus for broadcasting application events.
    ///
    /// # Returns
    /// A new `AppController` instance.
    pub fn new(
        ble_controller: BTCT,
        acq_controller: ACT,
        storage: Arc<RwLock<StorageModel<AcquisitionModel>>>,
        event_bus: Sender<AppEvent>,
    ) -> Self {
        trace!("Initializing AppController.");
        let (vtx, _) = tokio::sync::broadcast::channel(16);
        Self {
            view_tx: vtx.clone(),
            event_bus: event_bus.clone(),
            ble_controller,
            acq_controller,
            storage,
        }
    }

    /// Returns the view manager.
    ///
    /// # Returns
    /// A `ViewManager` instance.
    pub fn get_viewmanager(&self) -> ViewManager {
        ViewManager::new(self.view_tx.subscribe(), self.event_bus.clone())
    }

    /// Handles UI input events.
    ///
    /// # Arguments
    /// - `event`: The UI input event to handle.
    ///
    /// # Returns
    /// A result indicating success or failure.
    async fn ui_event(&mut self, event: UiInputEvent) -> Result<()> {
        match event {
            UiInputEvent::TimeWindowChanged(time) => {
                self.acq_controller.set_stats_window(&time).await?;
            }
            UiInputEvent::OutlierFilterChanged(val) => {
                self.acq_controller.set_outlier_filter_value(val).await?;
            }
            UiInputEvent::AcquisitionStopReq => {
                self.acq_controller.stop_acquisition()?;
                self.ble_controller.stop_listening().await?;
            }
            UiInputEvent::AcquisitionStartReq => {
                let m = self.acq_controller.start_acquisition()?;
                self.ble_controller.start_listening().await?;
                self.view_tx.send(ViewState::Acquisition((
                    m,
                    self.ble_controller.get_model()?,
                )))?;
            }
            UiInputEvent::StoredAcqSelected(idx) => {
                self.acq_controller.set_active_acq(idx).await?;
            }
            UiInputEvent::DiscardAcquisition => {
                self.ble_controller.stop_listening().await?;
                self.acq_controller.discard_acquisition()?;
                self.view_tx
                    .send(ViewState::Overview(self.storage.clone().into()))?;
            }
            UiInputEvent::StoreAcquisition => {
                self.ble_controller.stop_listening().await?;
                self.acq_controller.store_acquisition().await?;
                self.view_tx
                    .send(ViewState::Overview(self.storage.clone().into()))?;
            }
            UiInputEvent::SelectAdapter(adapter) => {
                self.ble_controller.select_adapter(&adapter).await?;
                self.ble_controller.start_scan().await?;
            }
            UiInputEvent::SelectPeripheral(peri) => {
                self.ble_controller.select_peripheral(&peri).await?;
                self.ble_controller.stop_scan().await?;
            }
            UiInputEvent::PrepareAcquisition => {
                let m = self.acq_controller.start_acquisition()?;
                self.view_tx.send(ViewState::Acquisition((
                    m,
                    self.ble_controller.get_model()?,
                )))?;
            }
            UiInputEvent::LoadModel(path) => {
                let json = fs::read_to_string(&path).await?;
                if let Ok(Ok(sm)) = tokio::task::spawn_blocking(move || {
                    let serde_result: Result<StorageModel<AcquisitionModel>, serde_json::Error> =
                        serde_json::from_str(json.as_str());
                    serde_result.map_err(|o| o.to_string())
                })
                .await
                {
                    *self.storage.write().await = sm;
                }
                self.view_tx
                    .send(ViewState::Overview(self.storage.clone().into()))?;
            }
            UiInputEvent::StoreModel(path) => {
                let _str = self.storage.clone();
                if let Ok(Ok(json)) = tokio::task::spawn_blocking(move || {
                    serde_json::to_string(&*_str.blocking_read())
                })
                .await
                {
                    if let Err(e) = fs::write(&path, json).await {
                        error!("failed to write storage to file: {:?}", e);
                    }
                } else {
                    error!("failed to serialize storage");
                }
            }
            UiInputEvent::NewModel => {
                self.view_tx
                    .send(ViewState::Overview(self.storage.clone().into()))?;
            }
        }
        Ok(())
    }

    /// Asynchronous event handler.
    ///
    /// Processes application-level events and delegates them to appropriate controllers.
    ///
    /// # Arguments
    /// - `gui_ctx`: The GUI context.
    pub async fn event_handler(mut self, gui_ctx: egui::Context) {
        let mut event_ch_rx = self.event_bus.subscribe();
        while let Err(e) = self
            .view_tx
            .send(ViewState::Overview(self.storage.clone().into()))
        {
            error!(
                "could not send initial viewstate, trying again in 5 sec: {}",
                e
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
        while let Err(e) = self.ble_controller.discover_adapters().await {
            error!(
                "could not discover adapters: {}. trying again in 5 seconds",
                e
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
        while let Ok(evt) = event_ch_rx.recv().await {
            match evt {
                AppEvent::Bluetooth(_btev) => {}
                AppEvent::UiInput(event) => {
                    if let Err(e) = self.ui_event(event).await {
                        error!(
                            "error during UiEvent handling: {}\n backtrace:\n{}",
                            e,
                            e.backtrace()
                        );
                    }
                }
            }
        }
        gui_ctx.request_repaint();
    }
}
