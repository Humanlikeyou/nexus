//! `<think>…</think>` removal, for whole strings and for streams where a tag can be split
//! across chunks.

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

/// Whole-string version (ported from Nexus `llm_router.rs`), extended to drop an unclosed
/// trailing block and orphan closing tags.
pub fn strip_thinking_tags(s: &str) -> String {
    let mut st = ThinkStripper::default();
    let mut out = st.push(s);
    out.push_str(&st.finish());
    out.trim().to_string()
}

/// Incremental stripper: feed chunks with `push`, then call `finish` once.
#[derive(Default, Debug)]
pub struct ThinkStripper {
    in_think: bool,
    /// Unemitted tail that may be the start of a tag.
    pending: String,
    /// Drop leading whitespace after a think block (models emit "\n\n" after `</think>`).
    trim_next: bool,
    /// Nothing visible emitted yet: leading whitespace is dropped too.
    started: bool,
}

/// Length of the longest suffix of `s` that is a proper prefix of `tag`.
fn partial_suffix(s: &str, tag: &str) -> usize {
    (1..tag.len())
        .rev()
        .find(|&n| {
            s.len() >= n && s.is_char_boundary(s.len() - n) && tag.starts_with(&s[s.len() - n..])
        })
        .unwrap_or(0)
}

impl ThinkStripper {
    pub fn push(&mut self, chunk: &str) -> String {
        let mut buf = std::mem::take(&mut self.pending);
        buf.push_str(chunk);
        let mut out = String::new();
        loop {
            if self.in_think {
                if let Some(i) = buf.find(CLOSE) {
                    buf.drain(..i + CLOSE.len());
                    self.in_think = false;
                    self.trim_next = true;
                    continue;
                }
                let keep = partial_suffix(&buf, CLOSE);
                self.pending = buf[buf.len() - keep..].to_string();
                break;
            }
            let open = buf.find(OPEN);
            let close = buf.find(CLOSE);
            match (open, close) {
                // An orphan closing tag before any opening tag: drop just the tag.
                (o, Some(c)) if o.is_none_or(|o| c < o) => {
                    self.emit(&mut out, &buf[..c]);
                    buf.drain(..c + CLOSE.len());
                    self.trim_next = true;
                }
                (Some(o), _) => {
                    self.emit(&mut out, &buf[..o]);
                    buf.drain(..o + OPEN.len());
                    self.in_think = true;
                }
                _ => {
                    let keep = partial_suffix(&buf, OPEN).max(partial_suffix(&buf, CLOSE));
                    let cut = buf.len() - keep;
                    self.emit(&mut out, &buf[..cut]);
                    self.pending = buf[cut..].to_string();
                    break;
                }
            }
        }
        out
    }

    fn emit(&mut self, out: &mut String, text: &str) {
        let text = if self.trim_next || !self.started {
            text.trim_start()
        } else {
            text
        };
        if !text.is_empty() {
            self.trim_next = false;
            self.started = true;
            out.push_str(text);
        }
    }

    /// Flushes a held partial tag that never completed. An unclosed think block is dropped.
    pub fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.pending);
        let mut out = String::new();
        if !self.in_think {
            self.emit(&mut out, &rest);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_string() {
        assert_eq!(
            strip_thinking_tags("<think>plan</think>\n\nAnswer"),
            "Answer"
        );
        assert_eq!(
            strip_thinking_tags("a<think>x</think>b<think>y</think>c"),
            "abc"
        );
        assert_eq!(strip_thinking_tags("no tags"), "no tags");
        assert_eq!(strip_thinking_tags("hidden</think>\nshown"), "hiddenshown");
        assert_eq!(strip_thinking_tags("Answer<think>never closed"), "Answer");
    }

    fn run(chunks: &[&str]) -> String {
        let mut s = ThinkStripper::default();
        let mut out: String = chunks.iter().map(|c| s.push(c)).collect();
        out.push_str(&s.finish());
        out
    }

    #[test]
    fn streaming_with_tags_split_across_chunks() {
        assert_eq!(
            run(&["<thi", "nk>secret plan</th", "ink>\n\nHel", "lo"]),
            "Hello"
        );
        assert_eq!(
            run(&["<", "t", "h", "i", "n", "k", ">", "x", "<", "/", "think", ">", "ok"]),
            "ok"
        );
        assert_eq!(run(&["Hi <think>a</think> there"]), "Hi there");
        assert_eq!(run(&["a < b and c <th"]), "a < b and c <th");
        assert_eq!(run(&["2 <", " 3"]), "2 < 3");
        assert_eq!(run(&["", "<think>", "", "</think>", "", "x"]), "x");
    }

    #[test]
    fn streaming_never_emits_thinking_text() {
        let text = "<think>do not leak this</think>Final answer.";
        for split in 1..text.len() {
            let (a, b) = text.split_at(split);
            let out = run(&[a, b]);
            assert_eq!(out, "Final answer.", "split at {split}");
        }
    }

    #[test]
    fn multibyte_text_is_safe() {
        assert_eq!(
            run(&["こんにちは<thi", "nk>考え</think>世界"]),
            "こんにちは世界"
        );
    }
}
