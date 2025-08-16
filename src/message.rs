use crate::aprs::packet::DataType;
use crate::aprs::{AprsPacket, CallSign};
use crate::router::{PacketSource, RoutedPacket};
use anyhow::Result;
use chrono::{DateTime, Utc};
use log::{debug, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

#[derive(Debug, Clone)]
struct PendingMessage {
    packet: AprsPacket,
    attempts: u8,
    last_attempt: DateTime<Utc>,
}

pub struct MessageHandler {
    mycall: String,
    pending_acks: Arc<RwLock<HashMap<String, PendingMessage>>>,
    received_messages: Arc<RwLock<HashMap<String, DateTime<Utc>>>>,
}

impl MessageHandler {
    pub fn new(mycall: String) -> Self {
        MessageHandler {
            mycall,
            pending_acks: Arc::new(RwLock::new(HashMap::new())),
            received_messages: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn run(
        self,
        mut rx: mpsc::Receiver<RoutedPacket>,
        tx: mpsc::Sender<RoutedPacket>,
    ) -> Result<()> {
        info!("Starting message handler for {}", self.mycall);

        // Start retry timer
        let pending_acks = self.pending_acks.clone();
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                retry_pending_messages(&pending_acks, &tx_clone).await;
            }
        });

        // Start cleanup task
        let received_messages = self.received_messages.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                cleanup_old_messages(&received_messages).await;
            }
        });

        while let Some(routed) = rx.recv().await {
            if routed.packet.data_type == DataType::Message {
                self.handle_message(routed, &tx).await?;
            }
        }

        Ok(())
    }

    async fn handle_message(
        &self,
        routed: RoutedPacket,
        tx: &mpsc::Sender<RoutedPacket>,
    ) -> Result<()> {
        let info = routed.packet.information.clone();

        // Parse message format ":ADDRESSEE:Message text{msgid"
        if !info.starts_with(':') || info.len() < 11 {
            return Ok(());
        }

        let addressee = info[1..10].trim();
        if addressee != self.mycall && !addressee.starts_with(&self.mycall) {
            return Ok(());
        }

        let remaining = info[11..].to_string();

        // Check if this is an ack or rej
        if remaining.starts_with("ack") || remaining.starts_with("rej") {
            self.handle_ack_rej(routed, &remaining).await?;
        } else {
            // Regular message
            self.handle_incoming_message(routed, &remaining, tx).await?;
        }

        Ok(())
    }

    async fn handle_incoming_message(
        &self,
        routed: RoutedPacket,
        message_text: &str,
        tx: &mpsc::Sender<RoutedPacket>,
    ) -> Result<()> {
        // Extract message ID if present
        let (text, msg_id) = if let Some(id_pos) = message_text.rfind('{') {
            let text = &message_text[..id_pos];
            let id = &message_text[id_pos + 1..];
            (text, Some(id))
        } else {
            (message_text, None)
        };

        info!("Received message from {}: {}", routed.packet.source, text);

        // Check for duplicate
        if let Some(msg_id) = msg_id {
            let msg_key = format!("{}:{}", routed.packet.source, msg_id);
            let mut received = self.received_messages.write().await;

            match received.entry(msg_key) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(Utc::now());
                }
                std::collections::hash_map::Entry::Occupied(_) => {
                    debug!("Duplicate message, resending ack");
                }
            }

            // Send acknowledgment
            let ack_text = format!(":{:<9}:ack{}", routed.packet.source.to_string(), msg_id);

            let ack_packet = AprsPacket::new(
                CallSign::parse(&self.mycall).unwrap_or(CallSign::new("N0CALL", 0)),
                CallSign::new("APRS", 0),
                ack_text,
            );

            info!("Sending ack to {}: {}", routed.packet.source, msg_id);

            let routed_ack = RoutedPacket {
                packet: ack_packet,
                source: PacketSource::Internal,
            };

            let _ = tx.send(routed_ack).await;
        }

        // Process special commands
        if text.trim().to_uppercase() == "?APRST" {
            // Send telemetry status
            self.send_status_reply(&routed.packet.source, tx).await?;
        }

        Ok(())
    }

    async fn handle_ack_rej(&self, routed: RoutedPacket, ack_text: &str) -> Result<()> {
        let is_ack = ack_text.starts_with("ack");
        let msg_id = &ack_text[3..];

        info!(
            "Received {} from {} for msg {}",
            if is_ack { "ACK" } else { "REJ" },
            routed.packet.source,
            msg_id
        );

        // Remove from pending
        let mut pending = self.pending_acks.write().await;
        pending.remove(msg_id);

        Ok(())
    }

    async fn send_status_reply(
        &self,
        to: &CallSign,
        tx: &mpsc::Sender<RoutedPacket>,
    ) -> Result<()> {
        let status = "aprstx daemon running";
        let msg_text = format!(":{:<9}:{}", to.to_string(), status);

        let packet = AprsPacket::new(
            CallSign::parse(&self.mycall).unwrap_or(CallSign::new("N0CALL", 0)),
            CallSign::new("APRS", 0),
            msg_text,
        );

        let routed = RoutedPacket {
            packet,
            source: PacketSource::Internal,
        };

        let _ = tx.send(routed).await;

        Ok(())
    }
}

async fn retry_pending_messages(
    pending_acks: &Arc<RwLock<HashMap<String, PendingMessage>>>,
    tx: &mpsc::Sender<RoutedPacket>,
) {
    let mut pending = pending_acks.write().await;
    let now = Utc::now();
    let mut to_remove = Vec::new();

    for (msg_id, pending_msg) in pending.iter_mut() {
        let elapsed = now.signed_duration_since(pending_msg.last_attempt);

        // Retry after 30 seconds
        if elapsed.num_seconds() >= 30 {
            if pending_msg.attempts >= 3 {
                warn!("Message {} failed after 3 attempts, giving up", msg_id);
                to_remove.push(msg_id.clone());
            } else {
                pending_msg.attempts += 1;
                pending_msg.last_attempt = now;

                info!(
                    "Retrying message {} (attempt {})",
                    msg_id, pending_msg.attempts
                );

                let routed = RoutedPacket {
                    packet: pending_msg.packet.clone(),
                    source: PacketSource::Internal,
                };

                let _ = tx.send(routed).await;
            }
        }
    }

    for msg_id in to_remove {
        pending.remove(&msg_id);
    }
}

async fn cleanup_old_messages(received_messages: &Arc<RwLock<HashMap<String, DateTime<Utc>>>>) {
    let mut messages = received_messages.write().await;
    let now = Utc::now();
    let max_age = chrono::Duration::hours(24);

    messages.retain(|_, time| now.signed_duration_since(*time) < max_age);

    debug!("Cleaned up old messages, {} remaining", messages.len());
}
