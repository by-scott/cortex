use std::collections::VecDeque;

pub struct DoomLoopDetector {
    threshold: usize,
    recent_calls: VecDeque<(String, String, String)>,
}

impl DoomLoopDetector {
    #[must_use]
    pub const fn new(threshold: usize) -> Self {
        Self {
            threshold,
            recent_calls: VecDeque::new(),
        }
    }

    /// Record a tool invocation (input side). Output hash filled later by `record_tool_result`.
    pub fn record_tool_call(&mut self, tool_name: &str, input: &str) {
        let hash = simple_hash(input);
        self.recent_calls
            .push_back((tool_name.to_string(), hash, String::new()));
        let cap = self.threshold * 2;
        while self.recent_calls.len() > cap {
            self.recent_calls.pop_front();
        }
    }

    /// Fill the output hash of the most recent entry.
    pub fn record_tool_result(&mut self, output: &str) {
        if let Some(last) = self.recent_calls.back_mut() {
            last.2 = simple_hash(output);
        }
    }

    /// Check for doom loop: N consecutive identical (tool, `input_hash`, `output_hash`) triples.
    #[must_use]
    pub fn check(&self) -> Option<String> {
        if self.recent_calls.len() < self.threshold {
            return None;
        }
        let last = self.recent_calls.back()?;
        let consecutive = self
            .recent_calls
            .iter()
            .rev()
            .take(self.threshold)
            .take_while(|entry| entry == &last)
            .count();
        if consecutive >= self.threshold {
            Some(format!(
                "doom loop detected: {} repeated {} times with identical input and output",
                last.0, consecutive
            ))
        } else {
            None
        }
    }

    pub fn reset(&mut self) {
        self.recent_calls.clear();
    }
}

fn simple_hash(input: &str) -> String {
    let truncated: String = input.chars().take(64).collect();
    truncated
}
