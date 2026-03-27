use chrono::DateTime;

use crate::api::client::NanitClient;
use crate::session::init_session_store;

pub async fn run(session_path: &str, baby_uid: &str, limit: u32) -> anyhow::Result<()> {
    let mut session = init_session_store(session_path);
    let client = NanitClient::new();
    let messages = client.fetch_messages(&mut session, baby_uid, limit).await?;

    if messages.is_empty() {
        println!("No messages found.");
        return Ok(());
    }

    println!("Messages for baby {baby_uid}:");
    for msg in &messages {
        let time = DateTime::from_timestamp(msg.time, 0)
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| msg.time.to_string());
        println!("  [{time}] {} (id: {})", msg.msg_type, msg.id);
    }
    Ok(())
}
