use prost::Message as ProstMessage;

use crate::proto;

/// Encode a protobuf Message to bytes.
pub fn encode_message(msg: &proto::Message) -> Vec<u8> {
    msg.encode_to_vec()
}

/// Decode bytes into a protobuf Message.
pub fn decode_message(data: &[u8]) -> Result<proto::Message, prost::DecodeError> {
    proto::Message::decode(data)
}

/// Build a keepalive message.
pub fn encode_keepalive() -> Vec<u8> {
    let msg = proto::Message {
        r#type: proto::message::Type::Keepalive as i32,
        request: None,
        response: None,
    };
    encode_message(&msg)
}

/// Build a request message.
#[allow(dead_code)]
pub fn encode_request(_id: i32, _req_type: proto::RequestType, request: proto::Request) -> Vec<u8> {
    let msg = proto::Message {
        r#type: proto::message::Type::Request as i32,
        request: Some(request),
        response: None,
    };
    encode_message(&msg)
}

/// Build a GET_SENSOR_DATA request.
pub fn build_get_sensor_data_request(id: i32) -> proto::Request {
    proto::Request {
        id,
        r#type: proto::RequestType::GetSensorData as i32,
        get_sensor_data: Some(proto::GetSensorData {
            all: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Build a PUT_STREAMING request.
pub fn build_put_streaming_request(
    id: i32,
    rtmp_url: &str,
    status: proto::streaming::Status,
) -> proto::Request {
    proto::Request {
        id,
        r#type: proto::RequestType::PutStreaming as i32,
        streaming: Some(proto::Streaming {
            id: proto::StreamIdentifier::Mobile as i32,
            status: status as i32,
            rtmp_url: rtmp_url.to_string(),
            attempts: Some(1),
        }),
        ..Default::default()
    }
}

/// Build a GET_STATUS request.
#[allow(dead_code)]
pub fn build_get_status_request(id: i32) -> proto::Request {
    proto::Request {
        id,
        r#type: proto::RequestType::GetStatus as i32,
        get_status: Some(proto::GetStatus {
            all: Some(true),
        }),
        ..Default::default()
    }
}

/// Build a GET_SETTINGS request.
#[allow(dead_code)]
pub fn build_get_settings_request(id: i32) -> proto::Request {
    proto::Request {
        id,
        r#type: proto::RequestType::GetSettings as i32,
        ..Default::default()
    }
}

/// Sensor type to display name.
pub fn sensor_type_name(sensor_type: i32) -> &'static str {
    match proto::SensorType::try_from(sensor_type) {
        Ok(proto::SensorType::Sound) => "Sound",
        Ok(proto::SensorType::Motion) => "Motion",
        Ok(proto::SensorType::Temperature) => "Temperature",
        Ok(proto::SensorType::Humidity) => "Humidity",
        Ok(proto::SensorType::Light) => "Light",
        Ok(proto::SensorType::Night) => "Night",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keepalive_round_trip() {
        let data = encode_keepalive();
        let msg = decode_message(&data).unwrap();
        assert_eq!(msg.r#type, proto::message::Type::Keepalive as i32);
        assert!(msg.request.is_none());
        assert!(msg.response.is_none());
    }

    #[test]
    fn request_round_trip() {
        let req = build_get_sensor_data_request(42);
        let data = encode_request(42, proto::RequestType::GetSensorData, req);
        let msg = decode_message(&data).unwrap();
        assert_eq!(msg.r#type, proto::message::Type::Request as i32);
        let request = msg.request.unwrap();
        assert_eq!(request.id, 42);
        assert_eq!(request.r#type, proto::RequestType::GetSensorData as i32);
        assert!(request.get_sensor_data.unwrap().all.unwrap());
    }

    #[test]
    fn streaming_round_trip() {
        let req = build_put_streaming_request(1, "rtmp://localhost/live/test", proto::streaming::Status::Started);
        let data = encode_request(1, proto::RequestType::PutStreaming, req);
        let msg = decode_message(&data).unwrap();
        let request = msg.request.unwrap();
        let streaming = request.streaming.unwrap();
        assert_eq!(streaming.rtmp_url, "rtmp://localhost/live/test");
        assert_eq!(streaming.status, proto::streaming::Status::Started as i32);
        assert_eq!(streaming.id, proto::StreamIdentifier::Mobile as i32);
    }
}
