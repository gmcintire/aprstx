use crate::aprs::AprsPacket;
use crate::config::Config;
use crate::filter::PacketFilter;
use crate::telemetry::TELEMETRY_STATS;
use anyhow::Result;
use log::{debug, info};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

#[derive(Debug, Clone, PartialEq)]
pub enum PacketSource {
    SerialPort(String),
    AprsIs,
    Internal,
}

#[derive(Debug, Clone)]
pub struct RoutedPacket {
    pub packet: AprsPacket,
    pub source: PacketSource,
}

pub struct PacketRouter {
    config: Arc<Config>,
    filter: Arc<PacketFilter>,
    rx_channel: mpsc::Receiver<RoutedPacket>,
    rf_tx: broadcast::Sender<RoutedPacket>,
    is_tx: broadcast::Sender<RoutedPacket>,
    digipeater_tx: mpsc::Sender<RoutedPacket>,
    message_tx: mpsc::Sender<RoutedPacket>,
    recent_packets: Arc<RwLock<Vec<(String, std::time::Instant)>>>,
}

impl PacketRouter {
    pub fn new(
        config: Arc<Config>,
        filter: Arc<PacketFilter>,
        rx_channel: mpsc::Receiver<RoutedPacket>,
    ) -> (Self, RouterChannels) {
        let (rf_tx, _) = broadcast::channel(100);
        let (is_tx, _) = broadcast::channel(100);
        let (digipeater_tx, digipeater_rx) = mpsc::channel(100);
        let (message_tx, message_rx) = mpsc::channel(100);

        let channels = RouterChannels {
            rf_tx: rf_tx.clone(),
            is_tx: is_tx.clone(),
            digipeater_rx,
            message_rx,
        };

        let router = PacketRouter {
            config,
            filter,
            rx_channel,
            rf_tx,
            is_tx,
            digipeater_tx,
            message_tx,
            recent_packets: Arc::new(RwLock::new(Vec::new())),
        };

        (router, channels)
    }

    pub async fn run(mut self) -> Result<()> {
        info!("Starting packet router");

        let mut cleanup_interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

        loop {
            tokio::select! {
                Some(routed_packet) = self.rx_channel.recv() => {
                    self.route_packet(routed_packet).await?;
                }
                _ = cleanup_interval.tick() => {
                    self.cleanup_recent_packets().await;
                }
            }
        }
    }

    async fn route_packet(&self, routed_packet: RoutedPacket) -> Result<()> {
        let packet_str = routed_packet.packet.to_string();
        debug!(
            "Routing packet from {:?}: {}",
            routed_packet.source, packet_str
        );

        // Check for duplicate packets (viscous delay)
        if self.is_duplicate(&packet_str).await {
            debug!("Dropping duplicate packet: {}", packet_str);
            return Ok(());
        }

        // Apply filters
        if !self.filter.should_pass(&routed_packet.packet) {
            debug!("Packet filtered out: {}", packet_str);
            return Ok(());
        }

        // Check for RFONLY or NOGATE
        let is_rf_only = routed_packet.packet.has_rfonly();
        let is_no_gate = routed_packet.packet.has_nogate();

        // Route based on source and packet properties
        match &routed_packet.source {
            PacketSource::SerialPort(_) => {
                // RF packet received
                TELEMETRY_STATS.packets_rx.fetch_add(1, Ordering::Relaxed);

                // Send to digipeater if enabled
                if self.config.digipeater.enabled
                    && self.digipeater_tx.send(routed_packet.clone()).await.is_ok()
                {
                    TELEMETRY_STATS
                        .packets_digipeated
                        .fetch_add(1, Ordering::Relaxed);
                }

                // Send to APRS-IS if I-gate is enabled and packet allows it
                if !is_rf_only && !is_no_gate {
                    if let Some(aprs_is) = &self.config.aprs_is {
                        if aprs_is.rx_enable {
                            info!("Gating to APRS-IS: {}", packet_str);
                            if self.is_tx.send(routed_packet.clone()).is_ok() {
                                TELEMETRY_STATS
                                    .packets_igate_rf_to_is
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }

                // Check for messages addressed to us
                if routed_packet.packet.destination.call == self.config.mycall {
                    let _ = self.message_tx.send(routed_packet.clone()).await;
                }
            }
            PacketSource::AprsIs => {
                // APRS-IS packet received

                // Send to RF if TX is enabled
                if let Some(aprs_is) = &self.config.aprs_is {
                    if aprs_is.tx_enable {
                        // Check if packet should be transmitted on RF
                        if self.should_gate_to_rf(&routed_packet.packet).await {
                            info!("Gating to RF: {}", packet_str);
                            if self.rf_tx.send(routed_packet.clone()).is_ok() {
                                TELEMETRY_STATS
                                    .packets_igate_is_to_rf
                                    .fetch_add(1, Ordering::Relaxed);
                                TELEMETRY_STATS.packets_tx.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            PacketSource::Internal => {
                // Internal packet (generated by us)

                // Send to RF
                if self.rf_tx.send(routed_packet.clone()).is_ok() {
                    TELEMETRY_STATS.packets_tx.fetch_add(1, Ordering::Relaxed);
                }

                // Send to APRS-IS
                if let Some(aprs_is) = &self.config.aprs_is {
                    if aprs_is.tx_enable {
                        let _ = self.is_tx.send(routed_packet.clone());
                    }
                }
            }
        }

        // Store packet hash for duplicate detection
        self.store_packet_hash(&packet_str).await;

        Ok(())
    }

    async fn is_duplicate(&self, packet_str: &str) -> bool {
        let hash = calculate_packet_hash(packet_str);
        let recent = self.recent_packets.read().await;
        let now = std::time::Instant::now();
        let viscous_delay =
            std::time::Duration::from_secs(self.config.digipeater.viscous_delay as u64);

        recent
            .iter()
            .any(|(h, t)| h == &hash && now.duration_since(*t) < viscous_delay)
    }

    async fn store_packet_hash(&self, packet_str: &str) {
        let hash = calculate_packet_hash(packet_str);
        let mut recent = self.recent_packets.write().await;
        recent.push((hash, std::time::Instant::now()));

        // Keep list size reasonable
        if recent.len() > 1000 {
            recent.drain(0..100);
        }
    }

    async fn cleanup_recent_packets(&self) {
        let mut recent = self.recent_packets.write().await;
        let now = std::time::Instant::now();
        let max_age = std::time::Duration::from_secs(300); // 5 minutes

        recent.retain(|(_, t)| now.duration_since(*t) < max_age);
    }

    async fn should_gate_to_rf(&self, packet: &AprsPacket) -> bool {
        // Don't gate packets that came from TCPIP (already on RF)
        if packet.path.iter().any(|p| p.call.contains("TCPIP")) {
            return false;
        }

        // Don't gate our own packets back to RF
        if packet.source.call == self.config.mycall {
            return false;
        }

        // Gate messages addressed to local stations
        // This is a simplified implementation - could be enhanced with
        // local station tracking
        true
    }
}

pub struct RouterChannels {
    pub rf_tx: broadcast::Sender<RoutedPacket>,
    pub is_tx: broadcast::Sender<RoutedPacket>,
    pub digipeater_rx: mpsc::Receiver<RoutedPacket>,
    pub message_rx: mpsc::Receiver<RoutedPacket>,
}

fn calculate_packet_hash(packet: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    packet.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}
