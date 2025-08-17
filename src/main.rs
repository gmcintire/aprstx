use anyhow::Result;
use clap::Parser;
use log::info;
use std::path::PathBuf;
use tokio::signal;

mod aprs;
mod beacon;
mod config;
mod digipeater;
mod filter;
mod gps;
mod message;
mod network;
mod router;
mod serial;
mod telemetry;

use config::Config;
use filter::PacketFilter;
use router::PacketRouter;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "/etc/aprstx.conf")]
    config: PathBuf,

    #[arg(short, long)]
    debug: bool,

    #[arg(short, long)]
    foreground: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(if args.debug {
        "debug"
    } else {
        "info"
    }))
    .init();

    info!("Starting aprstx daemon...");

    let config = match Config::load(&args.config) {
        Ok(config) => Arc::new(config),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };
    info!("Loaded configuration from {:?}", args.config);

    // Create packet filter
    let filter = Arc::new(PacketFilter::new(config.filters.clone())?);

    // Create main packet channel
    let (packet_tx, packet_rx) = mpsc::channel(1000);

    // Create router
    let (router, channels) = PacketRouter::new(config.clone(), filter, packet_rx);

    let mut handles = vec![];

    // Start router
    let handle = tokio::spawn(router.run());
    handles.push(handle);

    // Start serial ports
    for serial_config in &config.serial_ports {
        let tx = packet_tx.clone();
        let rf_rx = channels.rf_tx.subscribe();
        let handle = tokio::spawn(serial::run_serial_port(serial_config.clone(), tx, rf_rx));
        handles.push(handle);
    }

    // Start APRS-IS connection
    if let Some(aprs_is_config) = &config.aprs_is {
        let tx = packet_tx.clone();
        let is_rx = channels.is_tx.subscribe();
        let handle = tokio::spawn(network::run_aprs_is_connection(
            aprs_is_config.clone(),
            tx,
            is_rx,
        ));
        handles.push(handle);
    }

    // Start digipeater
    if config.digipeater.enabled {
        let tx = packet_tx.clone();
        let handle = tokio::spawn(digipeater::run_digipeater(
            config.digipeater.clone(),
            channels.digipeater_rx,
            tx,
        ));
        handles.push(handle);
    }

    // Start telemetry
    if config.telemetry.enabled {
        let tx = packet_tx.clone();
        let handle = tokio::spawn(telemetry::run_telemetry(
            config.telemetry.clone(),
            config.mycall.clone(),
            tx,
        ));
        handles.push(handle);
    }

    // Start message handler
    let message_handler = message::MessageHandler::new(config.mycall.clone());
    let tx = packet_tx.clone();
    let handle = tokio::spawn(message_handler.run(channels.message_rx, tx));
    handles.push(handle);

    // Start GPS if configured
    let gps_tracker = if let Some(gps_config) = &config.gps {
        let source = match gps_config.gps_type.as_str() {
            "serial" => {
                if let (Some(device), Some(baud)) = (&gps_config.device, gps_config.baud_rate) {
                    gps::GpsSource::SerialNmea(device.clone(), baud)
                } else {
                    gps::GpsSource::None
                }
            }
            "gpsd" => {
                let host = gps_config.host.as_deref().unwrap_or("localhost");
                let port = gps_config.port.unwrap_or(2947);
                gps::GpsSource::Gpsd(host.to_string(), port)
            }
            "fixed" => {
                if let Some(pos_str) = &gps_config.position {
                    match gps::parse_fixed_position(pos_str) {
                        Ok(pos) => gps::GpsSource::Fixed(pos),
                        Err(e) => {
                            log::error!("Invalid fixed position: {}", e);
                            gps::GpsSource::None
                        }
                    }
                } else {
                    gps::GpsSource::None
                }
            }
            _ => gps::GpsSource::None,
        };

        let tracker = Arc::new(gps::GpsTracker::new(source));
        let tracker_clone = tracker.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = tracker_clone.run().await {
                log::error!("GPS tracker error: {}", e);
                return Err(e);
            }
            Ok(())
        });
        handles.push(handle);
        Some(tracker)
    } else {
        None
    };

    // Start beacon if configured
    if let (Some(beacon_config), Some(gps)) = (&config.beacon, gps_tracker) {
        if beacon_config.enabled {
            let tx = packet_tx.clone();
            let beacon = beacon::BeaconService::new(beacon_config.clone(), gps);
            let handle = tokio::spawn(beacon.run(tx));
            handles.push(handle);
        }
    }

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down...");
        },
        _ = terminate => {
            info!("Received terminate signal, shutting down...");
        },
    }

    Ok(())
}
