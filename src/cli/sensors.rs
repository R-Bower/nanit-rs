use tokio::signal;
use tracing::error;

use crate::api::client::NanitClient;
use crate::session::init_session_store;
use crate::ws::codec::sensor_type_name;
use crate::ws::connection::NanitWebSocket;

pub async fn run(session_path: &str, baby_uid: &str) -> anyhow::Result<()> {
    let mut session = init_session_store(session_path);
    let client = NanitClient::new();

    client.maybe_authorize(&mut session, false).await?;

    let babies = client.ensure_babies(&mut session).await?;
    let baby = babies
        .iter()
        .find(|b| b.uid == baby_uid)
        .ok_or_else(|| anyhow::anyhow!("Baby with UID {baby_uid} not found"))?;

    let mut ws = NanitWebSocket::new(&baby.camera_uid, session.auth_token());
    let mut sensor_rx = ws.sensor_data_rx();

    ws.connect().await?;
    println!("Connected to camera {}", baby.camera_uid);

    // Request initial sensor data
    println!("Requesting sensor data...");
    match ws.get_sensor_data().await {
        Ok(response) => {
            for sensor in &response.sensor_data {
                let name = sensor_type_name(sensor.sensor_type);
                if let Some(vm) = sensor.value_milli {
                    println!("  {name}: {}", vm as f64 / 1000.0);
                } else if let Some(v) = sensor.value {
                    println!("  {name}: {v}");
                }
            }
        }
        Err(e) => error!("Failed to get sensor data: {e}"),
    }

    // Listen for pushed sensor data
    let sensor_task = tokio::spawn(async move {
        while let Ok(sensors) = sensor_rx.recv().await {
            for sensor in &sensors {
                let name = sensor_type_name(sensor.sensor_type);
                if let Some(vm) = sensor.value_milli {
                    println!("  {name}: {}", vm as f64 / 1000.0);
                } else if let Some(v) = sensor.value {
                    println!("  {name}: {v}");
                }
            }
        }
    });

    signal::ctrl_c().await?;
    println!("\nDisconnecting...");
    ws.disconnect().await;
    sensor_task.abort();

    Ok(())
}
