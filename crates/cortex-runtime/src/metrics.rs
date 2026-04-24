use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

/// Real-time metrics collector using atomic counters.
/// Thread-safe, zero contention on reads.
pub struct MetricsCollector {
    turn_count: AtomicU64,
    turn_errors: AtomicU64,
    tool_calls: AtomicU64,
    tool_errors: AtomicU64,
    total_input_tokens: AtomicU64,
    total_output_tokens: AtomicU64,
    last_turn_input_tokens: AtomicU64,
    last_turn_output_tokens: AtomicU64,
    last_call_input_tokens: AtomicU64,
    last_call_output_tokens: AtomicU64,
    memory_captures: AtomicU64,
    memory_recalls: AtomicU64,
    alerts_fired: AtomicU64,
    prompt_updates: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveMetrics {
    pub turn_count: u64,
    pub turn_errors: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub tool_success_rate: f64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub last_turn_input_tokens: u64,
    pub last_turn_output_tokens: u64,
    pub last_turn_tokens: u64,
    pub last_call_input_tokens: u64,
    pub last_call_output_tokens: u64,
    pub last_call_tokens: u64,
    pub memory_captures: u64,
    pub memory_recalls: u64,
    pub alerts_fired: u64,
    pub prompt_updates: u64,
}

impl MetricsCollector {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            turn_count: AtomicU64::new(0),
            turn_errors: AtomicU64::new(0),
            tool_calls: AtomicU64::new(0),
            tool_errors: AtomicU64::new(0),
            total_input_tokens: AtomicU64::new(0),
            total_output_tokens: AtomicU64::new(0),
            last_turn_input_tokens: AtomicU64::new(0),
            last_turn_output_tokens: AtomicU64::new(0),
            last_call_input_tokens: AtomicU64::new(0),
            last_call_output_tokens: AtomicU64::new(0),
            memory_captures: AtomicU64::new(0),
            memory_recalls: AtomicU64::new(0),
            alerts_fired: AtomicU64::new(0),
            prompt_updates: AtomicU64::new(0),
        }
    }

    pub fn record_turn(&self) {
        self.turn_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_turn_error(&self) {
        self.turn_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_tool_call(&self, is_error: bool) {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
        if is_error {
            self.tool_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_tokens(&self, input: u64, output: u64) {
        self.total_input_tokens.fetch_add(input, Ordering::Relaxed);
        self.total_output_tokens
            .fetch_add(output, Ordering::Relaxed);
        self.last_turn_input_tokens.store(input, Ordering::Relaxed);
        self.last_turn_output_tokens
            .store(output, Ordering::Relaxed);
    }

    pub fn record_last_call_tokens(&self, input: u64, output: u64) {
        self.last_call_input_tokens.store(input, Ordering::Relaxed);
        self.last_call_output_tokens
            .store(output, Ordering::Relaxed);
    }

    pub fn record_memory_capture(&self) {
        self.memory_captures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_memory_recall(&self) {
        self.memory_recalls.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_alert(&self) {
        self.alerts_fired.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_prompt_update(&self) {
        self.prompt_updates.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub fn snapshot(&self) -> LiveMetrics {
        let tool_calls = self.tool_calls.load(Ordering::Relaxed);
        let tool_errors = self.tool_errors.load(Ordering::Relaxed);
        let input = self.total_input_tokens.load(Ordering::Relaxed);
        let output = self.total_output_tokens.load(Ordering::Relaxed);
        let last_turn_input = self.last_turn_input_tokens.load(Ordering::Relaxed);
        let last_turn_output = self.last_turn_output_tokens.load(Ordering::Relaxed);
        let last_call_input = self.last_call_input_tokens.load(Ordering::Relaxed);
        let last_call_output = self.last_call_output_tokens.load(Ordering::Relaxed);

        LiveMetrics {
            turn_count: self.turn_count.load(Ordering::Relaxed),
            turn_errors: self.turn_errors.load(Ordering::Relaxed),
            tool_calls,
            tool_errors,
            tool_success_rate: if tool_calls > 0 {
                f64::from(u32::try_from(tool_calls - tool_errors).unwrap_or(u32::MAX))
                    / f64::from(u32::try_from(tool_calls).unwrap_or(u32::MAX))
            } else {
                1.0
            },
            total_input_tokens: input,
            total_output_tokens: output,
            total_tokens: input + output,
            last_turn_input_tokens: last_turn_input,
            last_turn_output_tokens: last_turn_output,
            last_turn_tokens: last_turn_input + last_turn_output,
            last_call_input_tokens: last_call_input,
            last_call_output_tokens: last_call_output,
            last_call_tokens: last_call_input + last_call_output,
            memory_captures: self.memory_captures.load(Ordering::Relaxed),
            memory_recalls: self.memory_recalls.load(Ordering::Relaxed),
            alerts_fired: self.alerts_fired.load(Ordering::Relaxed),
            prompt_updates: self.prompt_updates.load(Ordering::Relaxed),
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}
