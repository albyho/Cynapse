use super::*;

pub(super) async fn run_local_adapter(port: u16, token: String) {
    loop {
        let Ok((mut socket, _)) = connect_async(format!("ws://127.0.0.1:{port}/")).await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Ok(hello) = Envelope::hello(token.clone()).to_json() else {
            break;
        };
        if socket.send(Message::Text(hello.into())).await.is_err() {
            continue;
        }

        while let Some(message) = socket.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    let Ok(envelope) = Envelope::parse(text.as_str()) else {
                        continue;
                    };
                    let ack = match envelope.kind {
                        MessageType::Action
                            if envelope
                                .payload
                                .get("type")
                                .and_then(|value| value.as_str())
                                == Some("Present") =>
                        {
                            Envelope {
                                v: PROTOCOL_VERSION,
                                id: envelope.id,
                                kind: MessageType::Ack,
                                unit_path: None,
                                payload: serde_json::json!({ "status": "done" }),
                            }
                        }
                        MessageType::Action => Envelope::ack_error(
                            envelope.id,
                            "adapter_required",
                            "this Action requires an external App Adapter",
                        ),
                        MessageType::WorkspaceReset => Envelope {
                            v: PROTOCOL_VERSION,
                            id: envelope.id,
                            kind: MessageType::Ack,
                            unit_path: None,
                            payload: serde_json::json!({ "status": "done" }),
                        },
                        _ => continue,
                    };
                    let Ok(json) = ack.to_json() else {
                        continue;
                    };
                    if socket.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
