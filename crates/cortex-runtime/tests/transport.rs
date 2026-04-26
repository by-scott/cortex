use cortex_runtime::transport::TransportAdapter;
use cortex_types::{DeliveryPhase, OutboundBlock, OutboundMessage, OwnedScope};

fn message() -> OutboundMessage {
    let owner = cortex_types::AuthContext::new(
        cortex_types::TenantId::from_static("tenant-a"),
        cortex_types::ActorId::from_static("alice"),
        cortex_types::ClientId::from_static("telegram"),
    );
    let mut message = OutboundMessage::new(OwnedScope::private_for(&owner), DeliveryPhase::Final);
    message.push(OutboundBlock::Text {
        text: "## Title\n**complete** [link](https://example.invalid)".to_string(),
        markdown: true,
    });
    message
}

#[test]
fn telegram_preserves_markdown_packets() {
    let adapter = TransportAdapter::telegram();
    let plan = message().plan(adapter.capabilities());
    let packets = adapter.render(&plan);

    assert_eq!(packets.len(), 1);
    assert_eq!(
        packets[0].text.as_deref(),
        Some("## Title\n**complete** [link](https://example.invalid)")
    );
    assert!(packets[0].markdown);
}

#[test]
fn qq_receives_plain_text_without_markdown_markers() {
    let adapter = TransportAdapter::qq();
    let plan = message().plan(adapter.capabilities());
    let packets = adapter.render(&plan);

    assert_eq!(packets.len(), 1);
    assert_eq!(
        packets[0].text.as_deref(),
        Some("Title\ncomplete link (https://example.invalid)")
    );
    assert!(!packets[0].markdown);
}
