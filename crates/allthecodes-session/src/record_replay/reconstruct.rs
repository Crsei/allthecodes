use super::types::{MessageRecord, RecordItem, RecordLine, RecordedMessage, SessionMetaRecord};

#[derive(Debug, Clone, Default)]
pub struct ReconstructedRecordSession {
    pub session_id: String,
    pub metadata: Option<SessionMetaRecord>,
    pub messages: Vec<RecordedMessage>,
    pub last_seq: u64,
}

pub fn reconstruct_recorded_messages(lines: &[RecordLine]) -> ReconstructedRecordSession {
    let mut session = ReconstructedRecordSession::default();
    for line in lines {
        if session.session_id.is_empty() {
            session.session_id = line.session_id.clone();
        }
        session.last_seq = line.seq;
        match &line.item {
            RecordItem::SessionMeta(metadata) if session.metadata.is_none() => {
                session.metadata = Some(metadata.clone());
            }
            RecordItem::Message(MessageRecord { message }) => {
                session.messages.push(message.clone());
            }
            _ => {}
        }
    }
    session
}
