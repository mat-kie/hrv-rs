//! Application Controller
//!
//! This module defines the main controller responsible for orchestrating the application.
//! It initializes and manages other controllers and coordinates the overall application flow.

use super::acquisition::DataAcquisitionApi;
use crate::{
    controller::bluetooth::BluetoothApi,
    core::events::{AppEvent, BluetoothEvent, UiInputEvent},
    model::{acquisition::AcquisitionModel, storage::StorageModel},
    view::manager::{ViewManager, ViewState},
};

use log::{error, info};
use std::{marker::PhantomData, sync::Arc};
use tokio::fs;
use tokio::sync::{broadcast::Sender, RwLock};
use tokio::task::JoinHandle;

/// Main application controller.
///
/// This structure manages the lifecycle of other controllers and handles application-level events.
pub struct AppController<ACT: DataAcquisitionApi + Send + 'static, BTCT: BluetoothApi + 'static> {
    view_tx: Sender<ViewState>,
    event_tx: Sender<AppEvent>,
    _task_handle: JoinHandle<()>,
    _marker: PhantomData<(ACT, BTCT)>,
}

impl<ACT: DataAcquisitionApi + Send + 'static, BTCT: BluetoothApi + 'static>
    AppController<ACT, BTCT>
{
    /// Creates a new `AppController`.
    ///
    /// # Arguments
    /// - `bt_model`: The Bluetooth model.
    /// - `acq_model`: The acquisition model.
    /// - `ble_controller`: The Bluetooth controller.
    /// - `acq_controller`: The acquisition controller.
    ///
    /// # Returns
    /// A new `AppController` instance.
    pub fn new(
        ble_controller: BTCT,
        acq_controller: ACT,
        storage: Arc<RwLock<StorageModel<AcquisitionModel>>>,
        event_bus: Sender<AppEvent>,
        gui_ctx: egui::Context,
    ) -> Self {
        info!("Initializing AppController.");
        let (vtx, _) = tokio::sync::broadcast::channel(16);
        Self {
            view_tx: vtx.clone(),
            event_tx: event_bus.clone(),
            _task_handle: tokio::spawn(Self::event_handler(
                ble_controller,
                acq_controller,
                storage,
                vtx,
                event_bus.clone(),
                gui_ctx,
            )),
            _marker: Default::default(),
        }
    }

    pub fn get_viewmanager(&self) -> ViewManager {
        ViewManager::new(self.view_tx.subscribe(), self.event_tx.clone())
    }

    /// Asynchronous event handler.
    ///
    /// Processes application-level events and delegates them to appropriate controllers.
    ///
    /// # Arguments
    /// - `view_ch`: Sender for view updates.
    /// - `event_ch`: Receiver for application events.
    /// - `gui_ctx`: The GUI context.
    async fn event_handler(
        mut ble_controller: BTCT,
        mut acq_controller: ACT,
        storage: Arc<RwLock<StorageModel<AcquisitionModel>>>,
        view: Sender<ViewState>,
        event_bus: Sender<AppEvent>,
        gui_ctx: egui::Context,
    ) {
        let mut event_ch_rx = event_bus.subscribe();
        view.send(ViewState::Overview(storage.clone().into()));
        ble_controller.discover_adapters().await;
        while let Ok(evt) = event_ch_rx.recv().await {
            match evt {
                AppEvent::Bluetooth(btev) => {
                    //if let Err(e) = Self::bluetooth_handler(&mut ble_controller, btev).await {
                    //    error!("Bluetooth event error: {:?}", e);
                    //}
                }
                AppEvent::UiInput(event) => {
                    match event {
                        UiInputEvent::TimeWindowChanged(time) => {
                            acq_controller.set_stats_window(&time).await;
                        }
                        UiInputEvent::OutlierFilterChanged(val) => {
                            // TODO: Implement outlier filter update logic.
                            acq_controller.set_outlier_filter_value(val).await;
                        }

                        UiInputEvent::AcquisitionStopReq => {
                            acq_controller.stop_acquisition();
                            ble_controller.stop_listening().await;
                        }
                        UiInputEvent::AcquisitionStartReq => {
                            let m = acq_controller.start_acquisition();
                            ble_controller.start_listening().await;
                            view.send(ViewState::Acquisition((m, ble_controller.get_model())));
                        }
                        UiInputEvent::StoredAcqSelected(idx)=>{
                            acq_controller.set_active_acq(idx).await;
                        }
                        UiInputEvent::DiscardAcquisition => {
                            ble_controller.stop_listening().await;
                            acq_controller.discard_acquisition();                            
                            view.send(ViewState::Overview(storage.clone().into()));

                        }
                        UiInputEvent::StoreAcquisition => {
                            ble_controller.stop_listening().await;
                            acq_controller.store_acquisition().await;
                            view.send(ViewState::Overview(storage.clone().into()));
                        }
                        UiInputEvent::SelectAdapter(adapter) => {
                            ble_controller.select_adapter(&adapter).await;
                            ble_controller.start_scan().await;
                        }
                        UiInputEvent::SelectPeripheral(peri) => {
                            ble_controller.select_peripheral(&peri).await;
                            ble_controller.stop_scan().await;
                        }
                        UiInputEvent::PrepareAcquisition => {
                            let m = acq_controller.start_acquisition();
                            view.send(ViewState::Acquisition((m, ble_controller.get_model())));
                        }
                        UiInputEvent::LoadModel(path) => {
                            let json = fs::read_to_string(&path)
                                .await
                                .map_err(|e| e.to_string())
                                .unwrap();
                            //

                            if let Ok(Ok(sm)) = tokio::task::spawn_blocking(move || {
                                let serde_result: Result<
                                    StorageModel<AcquisitionModel>,
                                    serde_json::Error,
                                > = serde_json::from_str(json.as_str());
                                serde_result.map_err(|o| o.to_string())
                            })
                            .await
                            {
                                *storage.write().await = sm;
                            }
                            view.send(ViewState::Overview(storage.clone().into()));
                        }
                        UiInputEvent::StoreModel(path) => {
                            let _str = storage.clone();
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
                            view.send(ViewState::Overview(storage.clone().into()));
                        }
                    }
                }
            }
            gui_ctx.request_repaint();
        }
    }

    async fn bluetooth_handler(
        bt_controller: &mut BTCT,
        event: BluetoothEvent,
    ) -> Result<(), String> {
        Err("Unhandled BluetoothEvent".into())
    }
    async fn data_handler(acq_controller: &mut ACT, event: UiInputEvent) -> Result<(), String> {
        match event {
            UiInputEvent::TimeWindowChanged(time) => {
                acq_controller.set_stats_window(&time).await;
            }
            UiInputEvent::OutlierFilterChanged(val) => {
                // TODO: Implement outlier filter update logic.
                acq_controller.set_outlier_filter_value(val).await;
            }

            UiInputEvent::AcquisitionStopReq => {
                acq_controller.stop_acquisition();
            }
            _ => {}
        }
        Ok(())
    }
}
