//! Routing rules. First match wins:
//!
//! 0. Claude asked for (`/claude`, `X-Amara-Route: claude`, model `amara-claude`, mode hint
//!    `api`, or mode `hybrid` + an escalation tag). Used only when the cloud gate is open;
//!    otherwise the request goes deep (`claude_blocked=<why>`), as in the Nexus app before.
//! 1. `/deep`, `X-Amara-Route: deep`, model `amara-deep`, a trigger phrase, or an escalation
//!    tag (`heavy`/`research`/`code`/`long`) → deep. Commands and phrases are stripped.
//! 2. `/fast`, `X-Amara-Route: fast`, model `amara-fast` → fast (`/fast` stripped).
//! 3. Latest user message over the token threshold, or containing a ``` code block → deep.
//! 4. EASY/HARD classifier on the fast model (temperature 0, num_predict 3, timeout) → fast
//!    on EASY, deep on HARD, fast on timeout/error/unclear.

use crate::config::Routing;
use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Fast,
    Deep,
    Claude,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Fast => "fast",
            Tier::Deep => "deep",
            Tier::Claude => "claude",
        }
    }
}

/// Per-request hints from headers and the model name.
#[derive(Default, Clone, Debug)]
pub struct Hints {
    /// `X-Amara-Route` or a model alias.
    pub route: Option<Tier>,
    /// Where `route` came from (`header` or `model`), for the reason string.
    pub route_source: &'static str,
    /// `X-Amara-Allow-Claude: 0|1`. `Some(false)` forbids the cloud for this request.
    pub allow_claude: Option<bool>,
    /// `X-Amara-Tags` (comma-separated).
    pub tags: Vec<String>,
    /// `X-Amara-Mode`: `api` | `hybrid` | `local` | `amara`.
    pub mode: Option<String>,
}

impl Hints {
    pub fn from_model(mut self, model: &str) -> Self {
        if self.route.is_none() {
            let m = model.to_ascii_lowercase();
            let alias = if m.starts_with("amara-deep") {
                Some(Tier::Deep)
            } else if m.starts_with("amara-fast") {
                Some(Tier::Fast)
            } else if m.starts_with("amara-claude") {
                Some(Tier::Claude)
            } else {
                None
            };
            if alias.is_some() {
                self.route = alias;
                self.route_source = "model";
            }
        }
        self
    }
}

pub fn parse_tier(s: &str) -> Option<Tier> {
    match s.trim().to_ascii_lowercase().as_str() {
        "fast" | "light" => Some(Tier::Fast),
        "deep" => Some(Tier::Deep),
        "claude" | "anthropic" | "cloud" => Some(Tier::Claude),
        _ => None,
    }
}

const ESCALATION_TAGS: [&str; 4] = ["heavy", "research", "code", "long"];

/// Everything that must hold before a request may leave the Mac.
#[derive(Clone, Copy, Debug)]
pub struct CloudGate {
    pub enabled: bool,
    pub outbound_disabled: bool,
    pub has_key: bool,
    pub caller_allows: bool,
}

impl CloudGate {
    /// The first failing check, or None when Anthropic may be called.
    pub fn blocked(&self) -> Option<&'static str> {
        if self.outbound_disabled {
            Some("outbound_disabled")
        } else if !self.enabled {
            Some("cloud_disabled")
        } else if !self.caller_allows {
            Some("claude_not_allowed")
        } else if !self.has_key {
            Some("no_key")
        } else {
            None
        }
    }
}

/// Rough estimate: ~1 token per 4 characters (same as the Nexus app).
pub fn estimate_tokens(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    (text.len() as f64 / 4.0).ceil() as u32
}

/// A routing outcome before (or without) the classifier.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub tier: Tier,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PreRoute {
    Decided(Decision),
    Classify,
}

/// Removes a leading `/cmd` (whole word, any case). Returns the rest, or None.
fn strip_command(text: &str, cmd: &str) -> Option<String> {
    let t = text.trim_start();
    if t.len() < cmd.len() || !t[..cmd.len()].eq_ignore_ascii_case(cmd) {
        return None;
    }
    let rest = &t[cmd.len()..];
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim_start().to_string())
    } else {
        None
    }
}

/// Removes every case-insensitive occurrence of `phrase` and tidies the gap.
fn strip_phrase(text: &str, phrase: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let p = phrase.to_lowercase();
    if p.is_empty() || !lower.contains(&p) || lower.len() != text.len() {
        // Lowercasing changed byte lengths (rare non-ASCII): fall back to a plain search.
        return text
            .contains(phrase)
            .then(|| tidy(&text.replace(phrase, " ")));
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(off) = lower[i..].find(&p) {
        out.push_str(&text[i..i + off]);
        out.push(' ');
        i += off + p.len();
    }
    out.push_str(&text[i..]);
    Some(tidy(&out))
}

fn tidy(s: &str) -> String {
    let mut out = s
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    for p in [" ,", " .", " ?", " !", " :", " ;"] {
        out = out.replace(p, &p[1..]);
    }
    out.trim_matches(|c: char| c.is_whitespace() || c == ',' || c == ':' || c == ';')
        .to_string()
}

/// Rules 0-3. `text` is the latest user message; when a command or phrase is stripped the
/// cleaned text is written back into it.
pub fn pre_route(cfg: &Routing, text: &mut String, hints: &Hints, gate: &CloudGate) -> PreRoute {
    let decided = |tier, reason: String| PreRoute::Decided(Decision { tier, reason });

    // 0. Claude.
    let escalation = hints
        .tags
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .find(|t| ESCALATION_TAGS.contains(&t.as_str()));
    let mut claude_why = None;
    if let Some(rest) = strip_command(text, "/claude") {
        *text = rest;
        claude_why = Some("slash_claude".to_string());
    } else if hints.route == Some(Tier::Claude) {
        claude_why = Some(format!("{}_claude", hints.route_source));
    } else if hints.mode.as_deref() == Some("api") {
        claude_why = Some("mode=api".into());
    } else if hints.mode.as_deref() == Some("hybrid") && escalation.is_some() {
        claude_why = Some(format!(
            "mode=hybrid;tag={}",
            escalation.clone().unwrap_or_default()
        ));
    }
    if let Some(why) = claude_why {
        return match gate.blocked() {
            None => decided(Tier::Claude, why),
            Some(block) => decided(Tier::Deep, format!("{why}; claude_blocked={block}")),
        };
    }

    // 1. Deep.
    if let Some(rest) = strip_command(text, "/deep") {
        *text = rest;
        return decided(Tier::Deep, "slash_deep".into());
    }
    if hints.route == Some(Tier::Deep) {
        return decided(Tier::Deep, format!("{}_deep", hints.route_source));
    }
    for phrase in &cfg.trigger_phrases {
        if let Some(rest) = strip_phrase(text, phrase) {
            *text = rest;
            return decided(Tier::Deep, format!("trigger:{}", phrase.to_lowercase()));
        }
    }
    if let Some(tag) = escalation {
        return decided(Tier::Deep, format!("tag={tag}"));
    }

    // 2. Fast.
    if let Some(rest) = strip_command(text, "/fast") {
        *text = rest;
        return decided(Tier::Fast, "slash_fast".into());
    }
    if hints.route == Some(Tier::Fast) {
        return decided(Tier::Fast, format!("{}_fast", hints.route_source));
    }

    // 3. Size / code.
    if estimate_tokens(text) > cfg.token_threshold {
        return decided(Tier::Deep, "over_token_threshold".into());
    }
    if text.contains("```") {
        return decided(Tier::Deep, "code_block".into());
    }

    // 4.
    if cfg.classifier {
        PreRoute::Classify
    } else {
        decided(Tier::Fast, "default".into())
    }
}

/// Classifier prompt, sent as one user message: `{CLASSIFIER_PROMPT}{message}\nAnswer:`.
/// Short on purpose: on this Mac prompt evaluation runs at ~85 tokens/s and Gemma's sliding
/// window attention defeats prefix caching, so every token costs ~11 ms of routing time.
/// The reply is constrained to EASY/HARD with a JSON-schema enum.
pub const CLASSIFIER_PROMPT: &str =
    "EASY: chat, facts, smart-home commands, timers.\nHARD: planning, analysis, advice, maths, code, long writing.\nRequest: ";

/// Maps the classifier's raw reply to a decision.
pub fn classify_reply(reply: &str) -> Decision {
    let up = reply.to_ascii_uppercase();
    if up.contains("HARD") {
        Decision {
            tier: Tier::Deep,
            reason: "classifier_hard".into(),
        }
    } else if up.contains("EASY") {
        Decision {
            tier: Tier::Fast,
            reason: "classifier_easy".into(),
        }
    } else {
        Decision {
            tier: Tier::Fast,
            reason: "classifier_unclear".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> CloudGate {
        CloudGate {
            enabled: true,
            outbound_disabled: false,
            has_key: true,
            caller_allows: true,
        }
    }
    fn closed() -> CloudGate {
        CloudGate {
            enabled: false,
            ..open()
        }
    }
    fn route(text: &str, hints: Hints) -> (PreRoute, String) {
        let mut t = text.to_string();
        let r = pre_route(&Routing::default(), &mut t, &hints, &closed());
        (r, t)
    }
    fn d(tier: Tier, reason: &str) -> PreRoute {
        PreRoute::Decided(Decision {
            tier,
            reason: reason.into(),
        })
    }

    #[test]
    fn slash_deep_wins_and_is_stripped() {
        assert_eq!(
            route("/deep plan my week", Hints::default()),
            (d(Tier::Deep, "slash_deep"), "plan my week".into())
        );
        assert_eq!(route("  /DEEP\nplan", Hints::default()).1, "plan");
        // Not a command when glued to other text.
        assert_eq!(route("/deeply odd", Hints::default()).0, PreRoute::Classify);
    }

    #[test]
    fn trigger_phrase_goes_deep_and_is_stripped() {
        let (r, t) = route("Can you think deeper about my week?", Hints::default());
        assert_eq!(r, d(Tier::Deep, "trigger:think deeper"));
        assert_eq!(t, "Can you about my week?");
        let (r, t) = route("Think Harder: what is 2+2", Hints::default());
        assert_eq!(r, d(Tier::Deep, "trigger:think harder"));
        assert_eq!(t, "what is 2+2");
    }

    #[test]
    fn deep_beats_fast_and_fast_is_stripped() {
        // /deep is rule 1; a /fast later in text is not a leading command.
        assert_eq!(
            route("/deep /fast x", Hints::default()).0,
            d(Tier::Deep, "slash_deep")
        );
        assert_eq!(
            route("/fast what's 2+2", Hints::default()),
            (d(Tier::Fast, "slash_fast"), "what's 2+2".into())
        );
        // A trigger phrase beats /fast (rule 1 before rule 2).
        assert_eq!(
            route("/fast think deeper", Hints::default()).0,
            d(Tier::Deep, "trigger:think deeper")
        );
    }

    #[test]
    fn header_and_model_hints() {
        let h = Hints {
            route: Some(Tier::Deep),
            route_source: "header",
            ..Default::default()
        };
        assert_eq!(route("hi", h).0, d(Tier::Deep, "header_deep"));
        let h = Hints::default().from_model("amara-fast:latest");
        assert_eq!(
            route("explain relativity in depth", h).0,
            d(Tier::Fast, "model_fast")
        );
        let h = Hints::default().from_model("amara:latest");
        assert_eq!(route("hi", h).0, PreRoute::Classify);
        let h = Hints {
            tags: vec!["research".into()],
            ..Default::default()
        };
        assert_eq!(route("hi", h).0, d(Tier::Deep, "tag=research"));
    }

    #[test]
    fn token_threshold_and_code_block() {
        assert_eq!(
            route(&"a".repeat(8_004), Hints::default()).0,
            d(Tier::Deep, "over_token_threshold")
        );
        assert_eq!(
            route(&"a".repeat(8_000), Hints::default()).0,
            PreRoute::Classify
        );
        assert_eq!(
            route(
                "why does this fail?\n```rust\nfn main(){}\n```",
                Hints::default()
            )
            .0,
            d(Tier::Deep, "code_block")
        );
    }

    #[test]
    fn classifier_off_means_fast_default() {
        let cfg = Routing {
            classifier: false,
            ..Routing::default()
        };
        let mut t = "hello".to_string();
        assert_eq!(
            pre_route(&cfg, &mut t, &Hints::default(), &closed()),
            d(Tier::Fast, "default")
        );
    }

    #[test]
    fn classifier_reply_mapping() {
        assert_eq!(classify_reply("HARD").tier, Tier::Deep);
        assert_eq!(classify_reply(" easy.").tier, Tier::Fast);
        assert_eq!(classify_reply("hmm").reason, "classifier_unclear");
    }

    // ── cloud gate (ported from the Nexus ClaudeGate tests) ──

    type Break = fn(&mut CloudGate);

    #[test]
    fn gate_blocks_each_missing_condition() {
        assert_eq!(open().blocked(), None);
        let cases: [(Break, &str); 4] = [
            (|g| g.outbound_disabled = true, "outbound_disabled"),
            (|g| g.enabled = false, "cloud_disabled"),
            (|g| g.caller_allows = false, "claude_not_allowed"),
            (|g| g.has_key = false, "no_key"),
        ];
        for (break_it, want) in cases {
            let mut g = open();
            break_it(&mut g);
            assert_eq!(g.blocked(), Some(want));
            let mut t = "/claude hi".to_string();
            let r = pre_route(&Routing::default(), &mut t, &Hints::default(), &g);
            assert_eq!(
                r,
                d(Tier::Deep, &format!("slash_claude; claude_blocked={want}"))
            );
        }
    }

    #[test]
    fn claude_only_when_asked_and_open() {
        let mut t = "/claude hi".to_string();
        assert_eq!(
            pre_route(&Routing::default(), &mut t, &Hints::default(), &open()),
            d(Tier::Claude, "slash_claude")
        );
        assert_eq!(t, "hi");
        // Never asked → never Claude, even with an open gate.
        let mut t = "hi".to_string();
        assert_eq!(
            pre_route(&Routing::default(), &mut t, &Hints::default(), &open()),
            PreRoute::Classify
        );
        let api = Hints {
            mode: Some("api".into()),
            ..Default::default()
        };
        let mut t = "hi".to_string();
        assert_eq!(
            pre_route(&Routing::default(), &mut t, &api, &open()),
            d(Tier::Claude, "mode=api")
        );
        let hybrid = Hints {
            mode: Some("hybrid".into()),
            ..Default::default()
        };
        let mut t = "hi".to_string();
        assert_eq!(
            pre_route(&Routing::default(), &mut t, &hybrid, &open()),
            PreRoute::Classify
        );
    }

    #[test]
    fn estimate_tokens_rounds_up() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens(&"a".repeat(400)), 100);
    }
}
