use crate::api::client::NanitClient;
use crate::session::init_session_store;

pub async fn run(session_path: &str) -> anyhow::Result<()> {
    let mut session = init_session_store(session_path);
    let client = NanitClient::new();
    let babies = client.fetch_babies(&mut session).await?;

    if babies.is_empty() {
        println!("No babies found.");
        return Ok(());
    }

    println!("Babies:");
    for baby in &babies {
        println!("  {} (uid: {}, camera: {})", baby.name, baby.uid, baby.camera_uid);
    }
    Ok(())
}
