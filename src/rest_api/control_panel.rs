use anyhow::Result;
use futures::executor;
use log::error;
use parking_lot::Mutex;
use std::{sync::mpsc, sync::mpsc::Sender, sync::Arc, thread};

use super::endpoints;
use actix_web::{dev::Server, rt, App, HttpServer};
use tokio::sync::oneshot;

use crate::core::{
    lifecycle::{application_manager::ApplicationManager, trading_engine::Service},
    statistic_service::StatisticService,
};
use actix_web::web::Data;

pub(crate) struct ControlPanel {
    address: String,
    engine_settings: String,
    application_manager: Arc<ApplicationManager>,
    server_stopper_tx: Arc<Mutex<Option<Sender<()>>>>,
    work_finished_sender: Arc<Mutex<Option<oneshot::Sender<Result<()>>>>>,
    work_finished_receiver: Arc<Mutex<Option<oneshot::Receiver<Result<()>>>>>,
    statistics: Arc<StatisticService>,
}

impl ControlPanel {
    pub(crate) fn new(
        address: &str,
        engine_settings: String,
        application_manager: Arc<ApplicationManager>,
        statistics: Arc<StatisticService>,
    ) -> Arc<Self> {
        let (work_finished_sender, work_finished_receiver) = oneshot::channel();
        Arc::new(Self {
            address: address.to_owned(),
            engine_settings,
            application_manager,
            server_stopper_tx: Arc::new(Mutex::new(None)),
            work_finished_sender: Arc::new(Mutex::new(Some(work_finished_sender))),
            work_finished_receiver: Arc::new(Mutex::new(Some(work_finished_receiver))),
            statistics,
        })
    }

    /// Returned receiver will take a message when shutdown are completed
    pub(crate) fn stop(self: Arc<Self>) -> Option<oneshot::Receiver<Result<()>>> {
        if let Some(server_stopper_tx) = self.server_stopper_tx.lock().take() {
            if let Err(error) = server_stopper_tx.send(()) {
                error!("Unable to send signal to stop actix server: {}", error);
            }
        }

        let work_finished_receiver = self.work_finished_receiver.lock().take();
        work_finished_receiver
    }

    /// Start Actix Server in new thread
    pub(crate) fn start(self: Arc<Self>) -> Result<()> {
        let (server_stopper_tx, server_stopper_rx) = mpsc::channel::<()>();
        *self.server_stopper_tx.lock() = Some(server_stopper_tx.clone());
        let engine_settings = self.engine_settings.clone();
        let application_manager = self.application_manager.clone();
        let statistics = self.statistics.clone();
        let server = HttpServer::new(move || {
            App::new()
                .app_data(Data::new(server_stopper_tx.clone()))
                .app_data(Data::new(engine_settings.clone()))
                .app_data(Data::new(application_manager.clone()))
                .app_data(Data::new(statistics.clone()))
                .service(endpoints::health)
                .service(endpoints::stop)
                .service(endpoints::stats)
                .service(endpoints::get_config)
                .service(endpoints::set_config)
        })
        .bind(&self.address)?
        .shutdown_timeout(1)
        .workers(1)
        .run();

        let cloned_server = server.clone();
        self.clone()
            .server_stopping(cloned_server, server_stopper_rx);

        self.clone().start_server(server);

        Ok(())
    }

    fn server_stopping(self: Arc<Self>, server: Server, server_stopper_rx: mpsc::Receiver<()>) {
        let cloned_self = self.clone();
        thread::spawn(move || {
            if let Err(error) = server_stopper_rx.recv() {
                error!("Unable to receive signal to stop actix server: {}", error);
            }

            executor::block_on(server.stop(true));

            if let Some(work_finished_sender) = cloned_self.work_finished_sender.lock().take() {
                if let Err(_) = work_finished_sender.send(Ok(())) {
                    error!(
                        "Unable to send notification about server stopped. Probably receiver is already dropped",
                    );
                }
            }
        });
    }

    fn start_server(self: Arc<Self>, server: Server) {
        thread::spawn(move || {
            let system = Arc::new(rt::System::new());

            system.block_on(async {
                let _ = server;
            });
        });
    }
}

impl Service for ControlPanel {
    fn name(&self) -> &str {
        "ControlPanel"
    }

    fn graceful_shutdown(self: Arc<Self>) -> Option<oneshot::Receiver<Result<()>>> {
        self.clone().stop()
    }
}
