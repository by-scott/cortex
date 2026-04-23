use cortex_kernel::Journal;
use cortex_types::{CorrelationId, Event, Payload, TurnId};
use criterion::{Criterion, criterion_group, criterion_main};

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

fn bench_journal_append(c: &mut Criterion) {
    let tmp = must(tempfile::tempdir(), "benchmark tempdir should open");
    let journal = must(
        Journal::open(tmp.path().join("bench.db")),
        "benchmark journal should open",
    );

    c.bench_function("journal_append", |b| {
        b.iter(|| {
            let event = Event::new(TurnId::new(), CorrelationId::new(), Payload::TurnStarted);
            must(journal.append(&event), "benchmark append should succeed");
        });
    });
}

fn bench_journal_recent(c: &mut Criterion) {
    let tmp = must(tempfile::tempdir(), "benchmark tempdir should open");
    let journal = must(
        Journal::open(tmp.path().join("bench.db")),
        "benchmark journal should open",
    );

    for _ in 0..100 {
        let event = Event::new(TurnId::new(), CorrelationId::new(), Payload::TurnStarted);
        must(journal.append(&event), "benchmark append should succeed");
    }

    c.bench_function("journal_recent_50", |b| {
        b.iter(|| {
            must(
                journal.recent_events(50),
                "benchmark recent_events should succeed",
            );
        });
    });
}

criterion_group!(benches, bench_journal_append, bench_journal_recent);
criterion_main!(benches);
