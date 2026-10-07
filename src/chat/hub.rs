use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use super::protocol::ServerEvent;

/// Per-connection outbound buffer. A slow client that fills this is disconnected
/// so it cannot stall the group. Multi-instance fan-out later uses Redis pub/sub
/// into the same per-connection queues; this hub stays in-process for one replica.
pub const OUTBOUND_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ConnectionId(pub Uuid);

struct Connection {
    user_id: Uuid,
    groups: HashSet<Uuid>,
    tx: mpsc::Sender<Arc<str>>,
}

/// In-memory group rooms for a single API process.
///
/// Future horizontal scaling:
/// ```text
/// Instance A / B / C
///        │
///   Redis pub/sub (group event fan-out)
///        │
///   local ChatHub.broadcast (this type)
/// ```
pub struct ChatHub {
    inner: RwLock<Inner>,
}

struct Inner {
    connections: HashMap<ConnectionId, Connection>,
    groups: HashMap<Uuid, HashSet<ConnectionId>>,
}

impl ChatHub {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                connections: HashMap::new(),
                groups: HashMap::new(),
            }),
        }
    }

    pub async fn connect(&self, conn_id: ConnectionId, user_id: Uuid) -> mpsc::Receiver<Arc<str>> {
        let (tx, rx) = mpsc::channel(OUTBOUND_CAPACITY);
        let mut inner = self.inner.write().await;
        inner.connections.insert(
            conn_id,
            Connection {
                user_id,
                groups: HashSet::new(),
                tx,
            },
        );
        rx
    }

    pub async fn disconnect(&self, conn_id: ConnectionId) {
        let mut inner = self.inner.write().await;
        if let Some(conn) = inner.connections.remove(&conn_id) {
            for group_id in conn.groups {
                if let Some(members) = inner.groups.get_mut(&group_id) {
                    members.remove(&conn_id);
                    if members.is_empty() {
                        inner.groups.remove(&group_id);
                    }
                }
            }
        }
    }

    pub async fn subscribe(&self, conn_id: ConnectionId, group_id: Uuid) -> bool {
        let mut inner = self.inner.write().await;
        let Some(conn) = inner.connections.get_mut(&conn_id) else {
            return false;
        };
        conn.groups.insert(group_id);
        inner.groups.entry(group_id).or_default().insert(conn_id);
        true
    }

    pub async fn unsubscribe(&self, conn_id: ConnectionId, group_id: Uuid) {
        let mut inner = self.inner.write().await;
        if let Some(conn) = inner.connections.get_mut(&conn_id) {
            conn.groups.remove(&group_id);
        }
        if let Some(members) = inner.groups.get_mut(&group_id) {
            members.remove(&conn_id);
            if members.is_empty() {
                inner.groups.remove(&group_id);
            }
        }
    }

    pub async fn is_subscribed(&self, conn_id: ConnectionId, group_id: Uuid) -> bool {
        let inner = self.inner.read().await;
        inner
            .connections
            .get(&conn_id)
            .is_some_and(|conn| conn.groups.contains(&group_id))
    }

    /// Serialize once and fan out. Receivers in `skip_users` are omitted (block pairs).
    /// Slow connections are disconnected instead of blocking the group.
    pub async fn broadcast(&self, group_id: Uuid, event: &ServerEvent, skip_users: &HashSet<Uuid>) {
        let payload: Arc<str> = match serde_json::to_string(event) {
            Ok(json) => Arc::from(json),
            Err(_) => {
                tracing::error!("websocket event serialization failed");
                return;
            }
        };

        let mut slow = Vec::new();
        {
            let inner = self.inner.read().await;
            let Some(members) = inner.groups.get(&group_id) else {
                return;
            };
            for conn_id in members {
                let Some(conn) = inner.connections.get(conn_id) else {
                    continue;
                };
                if skip_users.contains(&conn.user_id) {
                    continue;
                }
                if conn.tx.try_send(payload.clone()).is_err() {
                    tracing::warn!(
                        connection_id = %conn_id.0,
                        user_id = %conn.user_id,
                        group_id = %group_id,
                        "dropping slow websocket connection"
                    );
                    slow.push(*conn_id);
                }
            }
        }
        for conn_id in slow {
            self.disconnect(conn_id).await;
        }
    }

    pub async fn send_to(&self, conn_id: ConnectionId, event: &ServerEvent) {
        let payload: Arc<str> = match serde_json::to_string(event) {
            Ok(json) => Arc::from(json),
            Err(_) => return,
        };
        let drop_slow = {
            let inner = self.inner.read().await;
            match inner.connections.get(&conn_id) {
                Some(conn) => conn.tx.try_send(payload).is_err(),
                None => false,
            }
        };
        if drop_slow {
            self.disconnect(conn_id).await;
        }
    }

    pub async fn connection_count(&self) -> usize {
        self.inner.read().await.connections.len()
    }

    pub async fn group_subscriber_count(&self, group_id: Uuid) -> usize {
        self.inner
            .read()
            .await
            .groups
            .get(&group_id)
            .map(HashSet::len)
            .unwrap_or(0)
    }
}

impl Default for ChatHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::protocol::ServerEvent;

    #[tokio::test]
    async fn subscribe_broadcast_and_disconnect_cleanup() {
        let hub = ChatHub::new();
        let group = Uuid::now_v7();
        let a = ConnectionId(Uuid::now_v7());
        let b = ConnectionId(Uuid::now_v7());
        let mut rx_a = hub.connect(a, Uuid::now_v7()).await;
        let mut rx_b = hub.connect(b, Uuid::now_v7()).await;
        assert!(hub.subscribe(a, group).await);
        assert!(hub.subscribe(b, group).await);
        assert_eq!(hub.group_subscriber_count(group).await, 2);

        hub.broadcast(
            group,
            &ServerEvent::Subscribed { group_id: group },
            &HashSet::new(),
        )
        .await;
        assert!(rx_a.recv().await.is_some());
        assert!(rx_b.recv().await.is_some());

        hub.disconnect(a).await;
        assert_eq!(hub.connection_count().await, 1);
        assert_eq!(hub.group_subscriber_count(group).await, 1);
        hub.disconnect(b).await;
        assert_eq!(hub.connection_count().await, 0);
        assert_eq!(hub.group_subscriber_count(group).await, 0);
    }

    #[tokio::test]
    async fn skip_users_are_not_notified() {
        let hub = ChatHub::new();
        let group = Uuid::now_v7();
        let blocked = Uuid::now_v7();
        let a = ConnectionId(Uuid::now_v7());
        let mut rx = hub.connect(a, blocked).await;
        hub.subscribe(a, group).await;
        let mut skip = HashSet::new();
        skip.insert(blocked);
        hub.broadcast(group, &ServerEvent::Subscribed { group_id: group }, &skip)
            .await;
        assert!(rx.try_recv().is_err());
    }
}
