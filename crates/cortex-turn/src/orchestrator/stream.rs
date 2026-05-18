pub(super) struct ThinkStreamFilter {
    strip: bool,
    in_think: bool,
    pending: String,
}

impl ThinkStreamFilter {
    pub(super) const fn new(strip: bool) -> Self {
        Self {
            strip,
            in_think: false,
            pending: String::new(),
        }
    }

    pub(super) fn push(&mut self, text: &str) -> String {
        if !self.strip {
            return text.to_string();
        }
        self.pending.push_str(text);
        let mut visible = String::new();
        loop {
            if self.in_think {
                if let Some(end) = self.pending.find("</think>") {
                    self.pending.drain(..end + "</think>".len());
                    self.in_think = false;
                    continue;
                }
                self.keep_possible_tag_suffix("</think>");
                return visible;
            }
            if let Some(start) = self.pending.find("<think>") {
                visible.push_str(&self.pending[..start]);
                self.pending.drain(..start + "<think>".len());
                self.in_think = true;
                continue;
            }
            let keep = possible_tag_suffix_len(&self.pending, "<think>");
            let emit_len = self.pending.len().saturating_sub(keep);
            visible.push_str(&self.pending[..emit_len]);
            self.pending.drain(..emit_len);
            return visible;
        }
    }

    pub(super) fn finish(&mut self) -> String {
        if !self.strip || self.in_think {
            self.pending.clear();
            return String::new();
        }
        std::mem::take(&mut self.pending)
    }

    fn keep_possible_tag_suffix(&mut self, tag: &str) {
        let keep = possible_tag_suffix_len(&self.pending, tag);
        if keep == 0 {
            self.pending.clear();
        } else {
            let keep_from = self.pending.len() - keep;
            self.pending.drain(..keep_from);
        }
    }
}

fn possible_tag_suffix_len(text: &str, tag: &str) -> usize {
    (1..tag.len())
        .rev()
        .find(|len| text.ends_with(&tag[..*len]))
        .unwrap_or(0)
}
