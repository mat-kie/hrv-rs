//! Application Controller
//!
//! This module defines the main controller responsible for orchestrating the application.
//! It initializes and manages other controllers and coordinates the overall application flow.

use super::acquisition::DataAcquisitionApi;
use crate::{
    controller::bluetooth::BluetoothApi,
    core::{
        events::{AppEvent, BluetoothEvent},
        view_trait::ViewApi,
    },
    model::{
        bluetooth::{AdapterHandle, BluetoothModelApi},
        storage::{StorageModel, StorageModelApi},
    },
    view::{bluetooth::BluetoothView, hrv_analysis::HrvView, model_initializer::ModelInitView, overview::StorageView},
};

use eframe::App;
use log::{error, info};
use nalgebra::storage;
use std::{marker::PhantomData, sync::Arc};
use tokio::sync::Mutex;
use tokio::{fs, sync::mpsc::Sender};
use tokio::{sync::mpsc::Receiver, task::JoinHandle};

/// Main application controller.
///
/// This structure manages the lifecycle of other controllers and handles application-level events.
pub struct AppController<
    AHT: AdapterHandle + Send + 'static,
    ACT: DataAcquisitionApi + Send + 'static,
    BTCT: BluetoothApi<AHT> + Send + 'static,
    BTMT: BluetoothModelApi<AHT> + Send + 'static,
    //ACMT: AcquisitionModelApi + Send + 'static,
> {
    view: Arc<Mutex<Box<dyn ViewApi>>>,
    storage : Arc<Mutex<StorageModel>>,
    _task_handle: JoinHandle<()>,
    /// Marker for type parameter `AHT`.
    _marker: PhantomData<AHT>,
    _marker1: PhantomData<ACT>,
    _marker2: PhantomData<BTCT>,
    _marker3: PhantomData<BTMT>,
}

impl<
        AHT: AdapterHandle + Send,
        ACT: DataAcquisitionApi + Send,
        BTCT: BluetoothApi<AHT> + Send,
        BTMT: BluetoothModelApi<AHT> + Send,
    > AppController<AHT, ACT, BTCT, BTMT>
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
        bt_model: Arc<Mutex<BTMT>>,
        mut ble_controller: BTCT,
        acq_controller: ACT,
        gui_ctx: egui::Context,
    ) -> Self {
        info!("Initializing AppController.");
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);
        ble_controller.initialize(event_tx.clone());
        let view: Arc<tokio::sync::Mutex<Box<dyn ViewApi>>> = Arc::new(tokio::sync::Mutex::new(
            Box::new(ModelInitView::new(event_tx.clone())),
        ));
        let _ = event_tx.try_send(AppEvent::Bluetooth(BluetoothEvent::DiscoverAdapters));
        let storage = Arc::new(Mutex::new(StorageModel::default()));
        Self {
            view: view.clone(),
            storage:storage.clone(),
            _task_handle: tokio::spawn(Self::event_handler(
                ble_controller,
                acq_controller,
                storage,
                view,
                event_rx,
                event_tx,
                gui_ctx,
            )),
            _marker: Default::default(),
            _marker1: Default::default(),
            _marker2: Default::default(),
            _marker3: Default::default(),
        }
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
        storage: Arc<Mutex<StorageModel>>,
        view: Arc<tokio::sync::Mutex<Box<dyn ViewApi>>>,
        mut event_ch_rx: Receiver<AppEvent>,
        event_ch_tx: Sender<AppEvent>,
        gui_ctx: egui::Context,
    ) {
        
        while let Some(evt) = event_ch_rx.recv().await {
            match evt {
                AppEvent::Bluetooth(btev) => {
                    if let Err(e) = ble_controller.handle_event(btev).await {
                        error!("Bluetooth event error: {:?}", e);
                    }
                }
                AppEvent::Data(hrev) => {
                    if let Err(e) = acq_controller.handle_event(hrev).await {
                        error!("Failed to handle HRV event: {}", e);
                    }
                }
                AppEvent::NewAcquisition => {
                    let _ = acq_controller.new_acquisition();
                    *view.lock().await = Box::new(HrvView::new(
                        acq_controller.get_acquisition(),
                        event_ch_tx.clone(),
                    ));
                }
                AppEvent::DiscardAcquisition => {
                    let _ = acq_controller.reset_acquisition().await;
                }
                AppEvent::LoadModel(path) => {
                    let json = fs::read_to_string(&path)
                        .await
                        .map_err(|e| e.to_string())
                        .unwrap();
                    //
                    if let Ok(model) = tokio::task::spawn_blocking(move || {
                        let storage: StorageModel = serde_json::from_str(&json)
                            .map_err(|e| e.to_string())
                            .unwrap();
                        storage
                    })
                    .await
                    {
                        *storage.lock().await = model;
                    }
                    *view.lock().await = Box::new(StorageView::new(storage.clone(), event_ch_tx.clone()));
                }
                AppEvent::StoreModel(path) => {
                    let _str = storage.clone();
                    if let Ok(Ok(json)) =
                        tokio::task::spawn_blocking(move || serde_json::to_string(&*_str.blocking_lock())).await
                    {
                        if let Err(e) = fs::write(&path, json).await {
                            error!("failed to write storage to file: {:?}", e);
                        }
                    } else {
                        error!("failed to serialize storage");
                    }
                }

                AppEvent::NewModel => {
                    //storage = Arc::new(Mutex::new(StorageModel::default()));
                    *view.lock().await = Box::new(StorageView::new(storage.clone(), event_ch_tx.clone()));
                }
                AppEvent::StoreAcquisition => {
                    let acq = acq_controller.get_acquisition();
                    storage.lock().await.store_acquisition(acq);
                    *view.lock().await = Box::new(StorageView::new(storage.clone(), event_ch_tx.clone()));
                    event_ch_tx.send(AppEvent::Bluetooth(BluetoothEvent::StopListening)).await;
                },
                AppEvent::SelectDevice=>{
                    *view.lock().await = Box::new(BluetoothView::new(ble_controller.get_model().clone(), event_ch_tx.clone()));
                }
            }
            gui_ctx.request_repaint();
        }
    }
}

impl<
        AHT: AdapterHandle + Send,
        ACT: DataAcquisitionApi + Send,
        BTCT: BluetoothApi<AHT> + Send,
        BTMT: BluetoothModelApi<AHT> + Send,
    > App for AppController<AHT, ACT, BTCT, BTMT>
{
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // TODO: make adjustable
        ctx.set_pixels_per_point(1.5);
        if let Err(e) = self.view.blocking_lock().render(ctx) {
            error!("Error during renderning: {}", e);
        }
    }
}
