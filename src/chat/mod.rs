//! Group chat WebSocket hub (single process).
//!
//! PostgreSQL is the source of truth. After reconnect, clients subscribe again
//! and call `GET /api/v1/groups/:id/messages?after=<lastId>` to recover gaps.
//!
//! Heartbeat: clients send JSON `{ "op": "ping", "ts": ... }`. Idle sockets
//! (no inbound frame for 90s) are closed. Dead connections are removed from rooms.
//!
//! Backpressure: each connection has a bounded outbound queue of 64 events.
//! A full queue disconnects that client only.
//!
//! Rate limits (in-process, per connection): 20 `message.send` and 60 total
//! ops per 10s window. Multi-instance production limiting must move to Redis.
//!
//! Horizontal scaling (not in this phase): instances publish group events to
//! Redis pub/sub; each instance fans out through this local hub.

mod handler;
mod hub;
mod protocol;

pub use handler::upgrade;
pub use hub::{ChatHub, ConnectionId, OUTBOUND_CAPACITY};
pub use protocol::{ClientOp, ServerEvent};
