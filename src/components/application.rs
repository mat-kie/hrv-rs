//! Application Controller
//!
//! This module defines the main controller responsible for orchestrating the application.
//! It initializes and manages other controllers and coordinates the overall application flow.

use crate::{
    api::{
        controller::{BluetoothApi, MeasurementApi, RecordingApi, StorageApi, StorageEventApi},
        model::{BluetoothModelApi, MeasurementModelApi, ModelHandle, StorageModelApi},
    },
    core::events::{AppEvent, StateChangeEvent},
    view::manager::{ViewManager, ViewState},
};

use anyhow::{anyhow, Result};
use log::{error, trace};
use std::{marker::PhantomData, sync::Arc};
use tokio::sync::{broadcast::Sender, RwLock};

/// Main application controller.
///
/// This structure manages the lifecycle of other controllers and handles application-level events.
pub struct AppController<
    MT: MeasurementApi + RecordingApi + 'static,
    ST: StorageApi<MT> + Send + 'static,
    BT: BluetoothApi + RecordingApi + 'static,
> {
    view_tx: Sender<ViewState>,
    event_bus: Sender<AppEvent>,
    ble_controller: Arc<RwLock<BT>>,
    acq_controller: Arc<RwLock<ST>>,
    active_measurement: Option<Arc<RwLock<MT>>>,
}

impl<
        MT: MeasurementApi + RecordingApi + Default + 'static,
        ST: StorageApi<MT> + StorageEventApi + StorageModelApi + Send + 'static,
        BT: BluetoothApi + RecordingApi + 'static,
    > AppController<MT, ST, BT>
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
    pub fn new(ble_controller: BT, acq_controller: ST, event_bus: Sender<AppEvent>) -> Self {
        trace!("Initializing AppController.");
        let (vtx, _) = tokio::sync::broadcast::channel(16);
        Self {
            view_tx: vtx.clone(),
            event_bus: event_bus.clone(),
            ble_controller: Arc::new(RwLock::new(ble_controller)),
            acq_controller: Arc::new(RwLock::new(acq_controller)),
            active_measurement: None,
        }
    }

    /// Returns the view manager.
    ///
    /// # Returns
    /// A `ViewManager` instance.
    pub fn get_viewmanager(&self) -> ViewManager {
        ViewManager::new(self.view_tx.subscribe(), self.event_bus.clone())
    }

    async fn handle_state_events(&mut self, event: StateChangeEvent) -> Result<()> {
        match event {
            StateChangeEvent::InitialState => {
                self.view_tx.send(ViewState::Overview((
                    {
                        let mh: Arc<RwLock<dyn StorageModelApi>> = self.acq_controller.clone();
                        ModelHandle::from(mh)
                    },
                    None,
                )))?;
            }
            StateChangeEvent::DiscardRecording => {
                self.active_measurement = None;
                self.view_tx.send(ViewState::Overview((
                    {
                        let mh: Arc<RwLock<dyn StorageModelApi>> = self.acq_controller.clone();
                        ModelHandle::from(mh)
                    },
                    None,
                )))?;
            }
            StateChangeEvent::StoreRecording => {
                if let Some(measurement) = self.active_measurement.as_ref() {
                    let mut lck = self.acq_controller.write().await;
                    lck.store_measurement(measurement.clone())?;
                    self.view_tx.send(ViewState::Overview((
                        ModelHandle::from(self.acq_controller.clone()),
                        Some(measurement.clone()),
                    )))?;
                }
            }
            StateChangeEvent::ToRecordingState => {
                // move to recording view
                let m: Arc<RwLock<MT>> = Arc::new(RwLock::new(MT::default()));
                self.active_measurement = Some(m.clone());
                let bm: ModelHandle<dyn BluetoothModelApi> = self.ble_controller.clone();
                self.view_tx.send(ViewState::Acquisition((m, bm)))?;
            }
            StateChangeEvent::SelectMeasurement(idx) => {
                let acq = self.acq_controller.read().await.get_measurement(idx)?;
                self.active_measurement = Some(acq.clone());
                self.view_tx.send(ViewState::Overview((
                    ModelHandle::from(self.acq_controller.clone()),
                    Some(acq.clone()),
                )))?;
            }
        }
        Ok(())
    }

    /// Dispatches application-level events to the appropriate controllers.
    async fn dispatch_event(&mut self, event: AppEvent) -> Result<()> {
        match event {
            AppEvent::Bluetooth(event) => {
                let mut lck = self.ble_controller.write().await;
                event.forward_to(&mut *lck).await
            }
            AppEvent::Measurement(event) => {
                if let Some(measurement) = self.active_measurement.as_ref() {
                    let mut lck = measurement.write().await;
                    event.forward_to(&mut *lck).await
                } else {
                    Ok(())
                }
            }
            AppEvent::Recording(event) => {
                if let Some(measurement) = self.active_measurement.as_ref() {
                    let mut lck = measurement.write().await;
                    if let Err(e) = event.clone().forward_to(&mut *lck).await {
                        return Err(e);
                    }
                }

                {
                    let mut ble_lock = self.ble_controller.write().await;
                    event.forward_to(&mut *ble_lock).await
                }
            }
            AppEvent::Storage(event) => {
                let mut lck = self.acq_controller.write().await;
                event.forward_to(&mut *lck).await
            }
            AppEvent::AppState(event) => self.handle_state_events(event).await,
        }
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
            .handle_state_events(StateChangeEvent::InitialState)
            .await
        {
            error!(
                "could not send initial viewstate, trying again in 5 sec: {}",
                e
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
        while let Err(e) = self.ble_controller.write().await.discover_adapters().await {
            error!(
                "could not discover adapters: {}. trying again in 5 seconds",
                e
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
        while let Ok(event) = event_ch_rx.recv().await {
            if let Err(e) = self.dispatch_event(event).await {
                error!(
                    "error during UiEvent handling: {}\nbacktrace:\n{}",
                    e,
                    e.backtrace()
                );
            }

            gui_ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::components::measurement::MeasurementData;
    use crate::core::events::{
        BluetoothEvent, MeasurementEvent, RecordingEvent, StateChangeEvent, StorageEvent,
    };
    use crate::model::bluetooth::{AdapterDescriptor, DeviceDescriptor};

    use async_trait::async_trait;
    use btleplug::api::BDAddr;
    use mockall::mock;
    use time::Duration;
    use tokio::sync::broadcast;
    mock! {
        Bluetooth {}
        impl std::fmt::Debug for Bluetooth{
            fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
        }
        #[async_trait]
        impl RecordingApi for Bluetooth{
            async fn start_recording(&mut self) -> Result<()>;
            async fn stop_recording(&mut self) -> Result<()>;
        }
        impl BluetoothModelApi for Bluetooth{
            fn get_adapters(&self) -> &[AdapterDescriptor];
            fn get_selected_adapter(&self) -> Option<AdapterDescriptor>;
            fn get_devices(&self) -> &Arc<RwLock<Vec<DeviceDescriptor>>>;
            fn get_selected_device(&self) -> Option<DeviceDescriptor>;
            fn is_scanning(&self) -> bool;
            fn is_listening_to(&self) -> Option<BDAddr>;
        }

        #[async_trait]
        impl BluetoothApi for Bluetooth{
            async fn discover_adapters(&mut self) -> Result<()>;
            async fn select_adapter(&mut self, adapter: AdapterDescriptor) -> Result<()>;
            async fn select_peripheral(&mut self, device: DeviceDescriptor) -> Result<()>;
            async fn start_scan(&mut self) -> Result<()>;
            async fn stop_scan(&mut self) -> Result<()>;
            async fn start_listening(&mut self) -> Result<()>;
            async fn stop_listening(&mut self) -> Result<()>;
        }
    }

    mock! {
        Storage{}
        impl std::fmt::Debug for Storage{
            fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
        }
        impl StorageModelApi for Storage{
            fn get_acquisitions(&self) -> &[ModelHandle<dyn MeasurementModelApi>];
        }

        impl StorageApi<MeasurementData> for Storage{
            fn get_measurement(& self, index:usize) -> Result<Arc<RwLock<MeasurementData>>>;
            fn store_measurement(&mut self, measurement: Arc<RwLock<MeasurementData>>) -> Result<()>;
        }

        #[async_trait]
        impl StorageEventApi for Storage{
            async fn clear(&mut self) -> Result<()>;
            async fn load_from_file(&mut self, path: PathBuf) -> Result<()>;
            async fn store_to_file(&mut self, path: PathBuf) -> Result<()>;
        }

        #[async_trait]
        impl RecordingApi for Storage{
            async fn start_recording(&mut self) -> Result<()>;
            async fn stop_recording(&mut self) -> Result<()>;
        }
    }

    #[tokio::test]
    async fn test_app_controller_initial_state() {
        let (event_bus_tx, _) = broadcast::channel(16);
        let ble_controller = MockBluetooth::new();
        let acq_controller = MockStorage::new();

        let mut app_controller =
            AppController::new(ble_controller, acq_controller, event_bus_tx.clone());
        let _vm = app_controller.get_viewmanager();
        let result = app_controller
            .handle_state_events(StateChangeEvent::InitialState)
            .await;
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn test_app_controller_recording_state() {
        let (event_bus_tx, _) = broadcast::channel(16);
        let ble_controller = MockBluetooth::new();
        let mut acq_controller = MockStorage::new();

        // Setup mocks
        let mock_measurement = Arc::new(RwLock::new(MeasurementData::default()));

        let mut app_controller =
            AppController::new(ble_controller, acq_controller, event_bus_tx.clone());
        let _vm = app_controller.get_viewmanager();

        let result = app_controller
            .handle_state_events(StateChangeEvent::ToRecordingState)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_app_controller_select_measurement() {
        let (event_bus_tx, _) = broadcast::channel(16);
        let ble_controller = MockBluetooth::new();
        let mut acq_controller = MockStorage::new();

        let mock_measurement = Arc::new(RwLock::new(MeasurementData::default()));
        let measurements = vec![{
            let m: Arc<RwLock<dyn MeasurementModelApi>> = mock_measurement.clone();
            m
        }];

        acq_controller
            .expect_get_acquisitions()
            .return_const(measurements);

        let mut app_controller =
            AppController::new(ble_controller, acq_controller, event_bus_tx.clone());
        let _vm = app_controller.get_viewmanager();

        let result = app_controller
            .handle_state_events(StateChangeEvent::SelectMeasurement(0))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_app_controller_bluetooth_event() {
        let (event_bus_tx, _) = broadcast::channel(16);
        let mut ble_controller = MockBluetooth::new();
        let acq_controller = MockStorage::new();
        let desc = AdapterDescriptor::new("MockAdapter".to_string());
        ble_controller
            .expect_discover_adapters()
            .returning(|| Ok(()));
        ble_controller.expect_select_adapter().returning(|_| Ok(()));

        let mut app_controller =
            AppController::new(ble_controller, acq_controller, event_bus_tx.clone());

        let event = AppEvent::Bluetooth(BluetoothEvent::SelectAdapter(desc.clone()));
        let result = app_controller.dispatch_event(event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_app_controller_measurement_event() {
        let (event_bus_tx, _) = broadcast::channel(16);
        let ble_controller = MockBluetooth::new();
        let mut acq_controller = MockStorage::new();

        let mock_measurement = Arc::new(RwLock::new(MeasurementData::default()));

        let mut app_controller =
            AppController::new(ble_controller, acq_controller, event_bus_tx.clone());

        let event = AppEvent::Measurement(MeasurementEvent::SetStatsWindow(Duration::minutes(1)));
        let result = app_controller.dispatch_event(event).await;
        assert!(result.is_ok());
        assert_eq!(
            mock_measurement.read().await.get_stats_window().unwrap(),
            &Duration::minutes(1)
        );
    }

    #[tokio::test]
    async fn test_app_controller_start_recording_event() {
        let (event_bus_tx, _) = broadcast::channel(16);
        let mut ble_controller = MockBluetooth::new();
        let mut acq_controller = MockStorage::new();

        acq_controller.expect_start_recording().returning(|| Ok(()));
        ble_controller.expect_start_recording().returning(|| Ok(()));

        let mut app_controller =
            AppController::new(ble_controller, acq_controller, event_bus_tx.clone());

        let event = AppEvent::Recording(RecordingEvent::StartRecording);
        let result = app_controller.dispatch_event(event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_app_controller_stop_recording_event() {
        let (event_bus_tx, _) = broadcast::channel(16);
        let mut ble_controller = MockBluetooth::new();
        let mut acq_controller = MockStorage::new();

        acq_controller.expect_stop_recording().returning(|| Ok(()));
        ble_controller.expect_stop_recording().returning(|| Ok(()));

        let mut app_controller =
            AppController::new(ble_controller, acq_controller, event_bus_tx.clone());

        let event = AppEvent::Recording(RecordingEvent::StopRecording);
        let result = app_controller.dispatch_event(event).await;
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn test_app_controller_storage_event() {
        let (event_bus_tx, _) = broadcast::channel(16);
        let ble_controller = MockBluetooth::new();
        let mut acq_controller = MockStorage::new();

        acq_controller.expect_clear().returning(|| Ok(()));

        let mut app_controller =
            AppController::new(ble_controller, acq_controller, event_bus_tx.clone());

        let event = AppEvent::Storage(StorageEvent::Clear);
        let result = app_controller.dispatch_event(event).await;
        assert!(result.is_ok());
    }
}
