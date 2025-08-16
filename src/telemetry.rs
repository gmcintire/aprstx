use crate::aprs::{AprsPacket, CallSign};
use crate::config::TelemetryConfig;
use crate::router::{PacketSource, RoutedPacket};
use anyhow::Result;
use log::info;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;

pub struct TelemetryStats {
    pub packets_rx: AtomicU64,
    pub packets_tx: AtomicU64,
    pub packets_digipeated: AtomicU64,
    pub packets_igate_rf_to_is: AtomicU64,
    pub packets_igate_is_to_rf: AtomicU64,
}

pub static TELEMETRY_STATS: TelemetryStats = TelemetryStats {
    packets_rx: AtomicU64::new(0),
    packets_tx: AtomicU64::new(0),
    packets_digipeated: AtomicU64::new(0),
    packets_igate_rf_to_is: AtomicU64::new(0),
    packets_igate_is_to_rf: AtomicU64::new(0),
};

pub async fn run_telemetry(
    config: TelemetryConfig,
    mycall: String,
    tx: mpsc::Sender<RoutedPacket>,
) -> Result<()> {
    info!(
        "Starting telemetry service with interval {}s",
        config.interval
    );

    let mut interval =
        tokio::time::interval(tokio::time::Duration::from_secs(config.interval as u64));
    let mut sequence = 0u32;

    loop {
        interval.tick().await;

        // Read statistics
        let rx_count = TELEMETRY_STATS.packets_rx.load(Ordering::Relaxed);
        let tx_count = TELEMETRY_STATS.packets_tx.load(Ordering::Relaxed);
        let digi_count = TELEMETRY_STATS.packets_digipeated.load(Ordering::Relaxed);
        let rf_to_is = TELEMETRY_STATS
            .packets_igate_rf_to_is
            .load(Ordering::Relaxed);
        let is_to_rf = TELEMETRY_STATS
            .packets_igate_is_to_rf
            .load(Ordering::Relaxed);

        // Create telemetry packet
        let telem_data = format!(
            "T#{:03},{:03},{:03},{:03},{:03},{:03},00000000",
            sequence % 1000,
            (rx_count % 256) as u8,
            (tx_count % 256) as u8,
            (digi_count % 256) as u8,
            (rf_to_is % 256) as u8,
            (is_to_rf % 256) as u8
        );

        let source = CallSign::parse(&mycall).unwrap_or(CallSign::new("N0CALL", 0));
        let packet = AprsPacket::new(source, CallSign::new("APRS", 0), telem_data);

        info!(
            "Sending telemetry: RX={}, TX={}, Digi={}, RF>IS={}, IS>RF={}",
            rx_count, tx_count, digi_count, rf_to_is, is_to_rf
        );

        let routed = RoutedPacket {
            packet,
            source: PacketSource::Internal,
        };

        let _ = tx.send(routed).await;

        // Send telemetry labels every 10 sequences
        if sequence % 10 == 0 {
            let labels = format!(":{:<9}:PARM.RxPkts,TxPkts,Digi,RF>IS,IS>RF", mycall);

            let label_packet = AprsPacket::new(
                CallSign::parse(&mycall).unwrap_or(CallSign::new("N0CALL", 0)),
                CallSign::new("APRS", 0),
                labels,
            );

            let routed_labels = RoutedPacket {
                packet: label_packet,
                source: PacketSource::Internal,
            };

            let _ = tx.send(routed_labels).await;

            // Send units
            let units = format!(":{:<9}:UNIT.Pkts,Pkts,Pkts,Pkts,Pkts", mycall);

            let unit_packet = AprsPacket::new(
                CallSign::parse(&mycall).unwrap_or(CallSign::new("N0CALL", 0)),
                CallSign::new("APRS", 0),
                units,
            );

            let routed_units = RoutedPacket {
                packet: unit_packet,
                source: PacketSource::Internal,
            };

            let _ = tx.send(routed_units).await;
        }

        // Also send a status message
        if !config.comment.is_empty() {
            let status = format!(">aprstx {}", config.comment);
            let status_packet = AprsPacket::new(
                CallSign::parse(&mycall).unwrap_or(CallSign::new("N0CALL", 0)),
                CallSign::new("APRS", 0),
                status,
            );

            let routed_status = RoutedPacket {
                packet: status_packet,
                source: PacketSource::Internal,
            };

            let _ = tx.send(routed_status).await;
        }

        sequence = sequence.wrapping_add(1);
    }
}
