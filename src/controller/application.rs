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
    },
    view::{bluetooth::BluetoothView, hrv_analysis::HrvView},
};

use eframe::App;
use log::{error, info};
use std::{marker::PhantomData, sync::Arc};
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;

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
        acq_model: Arc<Mutex<dyn AcquisitionModelApi>>,
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
        tokio::spawn(Self::event_handler(
            bt_model,
            acq_model,
            ble_controller,
            acq_controller,
            view.clone(),
            event_rx,
            event_tx,
            gui_ctx,
        ));
        Self {
            view,
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
        acq_model: Arc<Mutex<dyn AcquisitionModelApi>>,
        mut ble_controller: BTCT,
        mut acq_controller: ACT,
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

                    if bt_model.lock().await.is_listening_to().is_some() {
                        *view.lock().await =
                            Box::new(HrvView::new(acq_model.clone(), event_ch_tx.clone()));
                    } else {
                        *view.lock().await =
                            Box::new(BluetoothView::new(bt_model.clone(), event_ch_tx.clone()));
                    }
                }
                AppEvent::Data(hrev) => {
                    if let Err(e) = acq_controller.handle_event(hrev).await {
                        error!("Failed to handle HRV event: {}", e);
                    }
                }
                AppEvent::AcquisitionStartReq => {
                    acq_controller.new_acquisition();
                }
                AppEvent::AcquisitionStopReq(path) => {
                    if let Err(e) = acq_controller.store_acquisition(path) {
                        error!("Failed to store acquisition: {}", e);
                    }
                }
                AppEvent::SelectModel(model) => {}
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
        self.view.blocking_lock().render(ctx);
    }
}
