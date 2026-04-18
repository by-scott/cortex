//! Session persistence and restore integration tests.

use cortex_kernel::{Journal, SessionStore, project_message_history};
use cortex_types::{CorrelationId, Event, Message, Payload, SessionId, SessionMetadata, TurnId};

#[test]
fn session_persists_across_store_reopen() {
    let dir = tempfile::tempdir().unwrap();

    // Create and save session
    let sid = SessionId::new();
    {
        let store = SessionStore::open(dir.path()).unwrap();
        let meta = SessionMetadata::new(sid, 0);
        store.save(&meta).unwrap();

        // Save history
        let history = vec![Message::user("hello"), Message::assistant("hi there")];
        store.save_history(&sid, &history).unwrap();
    }

    // Reopen store (simulating restart)
    {
        let store = SessionStore::open(dir.path()).unwrap();
        let loaded = store.load(&sid).unwrap();
        assert_eq!(loaded.id, sid);
        assert!(loaded.is_active());

        let history = store.load_history(&sid);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].text_content(), "hello");
    }
}

#[test]
fn session_list_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    // Create multiple sessions
    {
        let store = SessionStore::open(dir.path()).unwrap();
        for i in 0u64..3 {
            let meta = SessionMetadata::new(SessionId::new(), i * 100);
            store.save(&meta).unwrap();
        }
    }

    // Reopen and verify
    {
        let store = SessionStore::open(dir.path()).unwrap();
        let sessions = store.list();
        assert_eq!(sessions.len(), 3);
    }
}

#[test]
fn journal_events_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let tid = TurnId::new();
    let cid = CorrelationId::new();

    // Write events
    {
        let journal = Journal::open(&db_path).unwrap();
        journal
            .append(&Event::new(tid, cid, Payload::TurnStarted))
            .unwrap();
        journal
            .append(&Event::new(
                tid,
                cid,
                Payload::UserMessage {
                    content: "test input".into(),
                },
            ))
            .unwrap();
        journal
            .append(&Event::new(
                tid,
                cid,
                Payload::AssistantMessage {
                    content: "test output".into(),
                },
            ))
            .unwrap();
        journal
            .append(&Event::new(tid, cid, Payload::TurnCompleted))
            .unwrap();
    }

    // Reopen and replay
    {
        let journal = Journal::open(&db_path).unwrap();
        let events = journal.recent_events(20).unwrap();
        assert_eq!(events.len(), 4);

        let history = project_message_history(&events);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].text_content(), "test input");
        assert_eq!(history[1].text_content(), "test output");
    }
}

#[test]
fn session_resume_from_journal_events() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let journal = Journal::open(&db_path).unwrap();
    let store = SessionStore::open(&dir.path().join("sessions")).unwrap();

    let tid = TurnId::new();
    let cid = CorrelationId::new();

    // Record session start offset
    let start_offset = journal.event_count().unwrap_or(0);

    // Simulate a session with events
    let sid = SessionId::new();
    journal
        .append(&Event::new(
            tid,
            cid,
            Payload::SessionStarted {
                session_id: sid.to_string(),
            },
        ))
        .unwrap();
    journal
        .append(&Event::new(
            tid,
            cid,
            Payload::UserMessage {
                content: "first question".into(),
            },
        ))
        .unwrap();
    journal
        .append(&Event::new(
            tid,
            cid,
            Payload::AssistantMessage {
                content: "first answer".into(),
            },
        ))
        .unwrap();

    let end_offset = journal.event_count().unwrap_or(0) + 1; // +1 because range is half-open [start, end)

    // Save session metadata with offset range
    let mut meta = SessionMetadata::new(sid, start_offset);
    meta.end_offset = Some(end_offset);
    meta.turn_count = 1;
    store.save(&meta).unwrap();

    // Session is restored by replaying events in the offset range
    let events = journal.events_in_range(start_offset, end_offset).unwrap();
    let history = project_message_history(&events);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].text_content(), "first question");
}
