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
        acquisition::AcquisitionModelApi,
        bluetooth::{AdapterHandle, BluetoothModelApi},
        storage::{StorageModel, StorageModelApi},
    },
    view::{bluetooth::BluetoothView, hrv_analysis::HrvView},
};

use eframe::App;
use log::{error, info};
use std::{marker::PhantomData, sync::Arc};
use tokio::{fs, sync::mpsc::Sender};
use tokio::sync::Mutex;
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
    view: Arc<tokio::sync::Mutex<Box<dyn ViewApi>>>,
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
        bt_model: Arc<tokio::sync::Mutex<BTMT>>,
        mut ble_controller: BTCT,
        acq_controller: ACT,
        gui_ctx: egui::Context,
    ) -> Self {
        info!("Initializing AppController.");
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);
        ble_controller.initialize(event_tx.clone());
        let view: Arc<tokio::sync::Mutex<Box<dyn ViewApi>>> = Arc::new(tokio::sync::Mutex::new(
            Box::new(BluetoothView::new(bt_model.clone(), event_tx.clone())),
        ));
        let _ = event_tx.try_send(AppEvent::Bluetooth(BluetoothEvent::DiscoverAdapters));
        Self {
            view: view.clone(),
            _task_handle: tokio::spawn(Self::event_handler(
                bt_model,
                ble_controller,
                acq_controller,
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
        bt_model: Arc<tokio::sync::Mutex<BTMT>>,
        mut ble_controller: BTCT,
        mut acq_controller: ACT,
        view: Arc<tokio::sync::Mutex<Box<dyn ViewApi>>>,
        mut event_ch_rx: Receiver<AppEvent>,
        event_ch_tx: Sender<AppEvent>,
        gui_ctx: egui::Context,
    ) {
        let mut storage = StorageModel::default();
        while let Some(evt) = event_ch_rx.recv().await {
            match evt {
                AppEvent::Bluetooth(btev) => {
                    if let Err(e) = ble_controller.handle_event(btev).await {
                        error!("Bluetooth event error: {:?}", e);
                    }

                    // if bt_model.lock().await.is_listening_to().is_some() {
                    //     else {
                    //     *view.lock().await =
                    //         Box::new(BluetoothView::new(bt_model.clone(), event_ch_tx.clone()));
                    // }
                }
                AppEvent::Data(hrev) => {
                    if let Err(e) = acq_controller.handle_event(hrev).await {
                        error!("Failed to handle HRV event: {}", e);
                    }
                }
                AppEvent::NewAcquisition => {
                    let _ =acq_controller.reset_acquisition().await;
                    *view.lock().await = Box::new(HrvView::new(acq_controller.get_acquisition(), event_ch_tx.clone()));
                    
                }
                AppEvent::DiscardAcquisition => {
                    // TODO
                }
                AppEvent::LoadModel(path) => {
                    let json = fs::read_to_string(&path).await.map_err(|e| e.to_string()).unwrap();
                    //
                    if let Ok(model) = tokio::task::spawn_blocking(move | | {
                        let storage: StorageModel = serde_json::from_str(&json).map_err(|e| e.to_string()).unwrap();
                        storage
                    }).await{
                     storage = model;   
                    }
                }
                AppEvent::StoreModel(path) => {
                    let _str = storage.clone();
                    if let Ok(Ok(json)) = tokio::task::spawn_blocking( move | | {
                        serde_json::to_string(&_str).map_err(|e| e.to_string())
                    }).await{
                        fs::write(&path, json).await.map_err(|e| e.to_string());
                    }

                }

                AppEvent::NewModel => {
                    storage = StorageModel::default();
                }
                AppEvent::StoreAcquisition => {
                    let acq = acq_controller.get_acquisition();
                    // storage.store_acquisition(acq);
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
