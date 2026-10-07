use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::dto::{GroupMessageDto, ReactionStateDto};

#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
pub enum ClientOp {
    #[serde(rename = "subscribe")]
    Subscribe {
        #[serde(rename = "groupId")]
        group_id: Uuid,
    },
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        #[serde(rename = "groupId")]
        group_id: Uuid,
    },
    #[serde(rename = "message.send")]
    MessageSend {
        #[serde(rename = "groupId")]
        group_id: Uuid,
        #[serde(rename = "clientMsgId")]
        client_msg_id: Uuid,
        content: String,
        #[serde(rename = "messageType")]
        message_type: Option<String>,
    },
    #[serde(rename = "reaction.toggle")]
    ReactionToggle {
        #[serde(rename = "messageId")]
        message_id: Uuid,
        emoji: String,
    },
    #[serde(rename = "message.edit")]
    MessageEdit {
        #[serde(rename = "messageId")]
        message_id: Uuid,
        content: String,
    },
    #[serde(rename = "message.delete")]
    MessageDelete {
        #[serde(rename = "messageId")]
        message_id: Uuid,
    },
    #[serde(rename = "ping")]
    Ping { ts: i64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op")]
pub enum ServerEvent {
    #[serde(rename = "subscribed")]
    Subscribed {
        #[serde(rename = "groupId")]
        group_id: Uuid,
    },
    #[serde(rename = "message.created")]
    MessageCreated { payload: GroupMessageDto },
    #[serde(rename = "message.edited")]
    MessageEdited { payload: GroupMessageDto },
    #[serde(rename = "message.deleted")]
    MessageDeleted { payload: GroupMessageDto },
    #[serde(rename = "reaction.updated")]
    ReactionUpdated { payload: ReactionStateDto },
    #[serde(rename = "error")]
    Error { code: String, message: String },
    #[serde(rename = "pong")]
    Pong { ts: i64 },
}

impl ServerEvent {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}
