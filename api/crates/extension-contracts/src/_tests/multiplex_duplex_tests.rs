use crate::{MultiplexHostMessage, MultiplexWorkerMessage};
use serde_json::json;

#[test]
fn legacy_event_wire_keeps_its_original_shape() {
    let value = json!({"kind":"event","call_id":"1","event":{"text":"hello"}});
    let frame: MultiplexWorkerMessage = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(frame).unwrap(), value);
}

#[test]
fn duplex_input_and_event_credit_have_canonical_correlations() {
    let input = MultiplexHostMessage::Input {
        call_id: "1".into(),
        input: json!({"data":"hello"}),
    };
    let ack = MultiplexHostMessage::EventAck {
        call_id: "1".into(),
        event_id: "2".into(),
    };
    for frame in [input, ack] {
        assert_eq!(
            serde_json::from_value::<MultiplexHostMessage>(serde_json::to_value(&frame).unwrap())
                .unwrap(),
            frame
        );
    }
    let frame = MultiplexWorkerMessage::Event {
        call_id: "1".into(),
        event: json!("output"),
        await_ack: true,
        event_id: Some("2".into()),
    };
    assert_eq!(
        serde_json::from_value::<MultiplexWorkerMessage>(serde_json::to_value(&frame).unwrap())
            .unwrap(),
        frame
    );
    assert!(serde_json::from_value::<MultiplexHostMessage>(
        json!({"kind":"input","call_id":"1","input":null,"actor_id":"forged"})
    )
    .is_err());
}
