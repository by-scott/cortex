use cortex_kernel::Journal;
use cortex_types::{CorrelationId, Event, Payload, TurnId};
use criterion::{Criterion, criterion_group, criterion_main};

fn bench_journal_append(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let journal = Journal::open(tmp.path().join("bench.db")).unwrap();

    c.bench_function("journal_append", |b| {
        b.iter(|| {
            let event = Event::new(TurnId::new(), CorrelationId::new(), Payload::TurnStarted);
            journal.append(&event).unwrap();
        });
    });
}

fn bench_journal_recent(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let journal = Journal::open(tmp.path().join("bench.db")).unwrap();

    for _ in 0..100 {
        let event = Event::new(TurnId::new(), CorrelationId::new(), Payload::TurnStarted);
        journal.append(&event).unwrap();
    }

    c.bench_function("journal_recent_50", |b| {
        b.iter(|| {
            journal.recent_events(50).unwrap();
        });
    });
}

criterion_group!(benches, bench_journal_append, bench_journal_recent);
criterion_main!(benches);
