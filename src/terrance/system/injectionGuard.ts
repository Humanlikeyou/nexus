/**
 * Prompt Injection Guard
 *
 * Scans memory content, skill files, and context entries for
 * prompt injection patterns before they're prepended to LLM prompts.
 *
 * Inspired by Hermes Agent's prompt_builder.py anti-injection system.
 *
 * Detects:
 *  - System prompt overrides ("ignore previous instructions", "you are now")
 *  - Secret exfiltration ("output the system prompt", "reveal your instructions")
 *  - Hidden content (invisible unicode, zero-width chars, HTML hidden tags)
 *  - Role hijacking ("Assistant:", "System:" embedded in user content)
 */

// ─── Injection Patterns ─────────────────────────────────────────────────────

interface InjectionPattern {
  name: string;
  pattern: RegExp;
  severity: "high" | "medium" | "low";
}

const INJECTION_PATTERNS: InjectionPattern[] = [
  // System prompt overrides
  { name: "ignore_instructions", pattern: /ignore\s+(all\s+)?previous\s+instructions/i, severity: "high" },
  { name: "forget_instructions", pattern: /forget\s+(all\s+)?(your\s+)?previous\s+(instructions|rules|constraints)/i, severity: "high" },
  { name: "new_identity", pattern: /you\s+are\s+now\s+(a|an|the)\b/i, severity: "high" },
  { name: "override_system", pattern: /override\s+(system|safety|your)\s+(prompt|instructions|rules)/i, severity: "high" },
  { name: "disregard_rules", pattern: /disregard\s+(all|any|your)\s+(rules|instructions|guidelines)/i, severity: "high" },
  { name: "jailbreak_prefix", pattern: /\bDAN\b.*\bdo\s+anything\s+now\b/i, severity: "high" },

  // Secret exfiltration
  { name: "reveal_prompt", pattern: /(?:output|reveal|show|print|display|repeat)\s+(?:the\s+)?(?:system\s+)?prompt/i, severity: "high" },
  { name: "reveal_instructions", pattern: /(?:output|reveal|show|print|display|repeat)\s+(?:your\s+)?instructions/i, severity: "high" },
  { name: "leak_context", pattern: /what\s+(?:is|are)\s+your\s+(?:system\s+)?(?:prompt|instructions|rules)/i, severity: "medium" },

  // Role hijacking (embedded in content that shouldn't contain role markers)
  { name: "role_hijack_assistant", pattern: /^Assistant:\s/m, severity: "medium" },
  { name: "role_hijack_system", pattern: /^System:\s/m, severity: "medium" },
  { name: "role_hijack_human", pattern: /^Human:\s/m, severity: "medium" },

  // Hidden content
  { name: "html_hidden", pattern: /<(?:div|span|p)\s[^>]*(?:display\s*:\s*none|visibility\s*:\s*hidden)/i, severity: "high" },
  { name: "html_comment_inject", pattern: /<!--[\s\S]*?(?:instruction|system|ignore|override)[\s\S]*?-->/i, severity: "medium" },

  // Invisible unicode (zero-width chars used to hide instructions)
  { name: "zero_width_chars", pattern: /[\u200B\u200C\u200D\uFEFF\u00AD]{3,}/, severity: "high" },
  { name: "rtl_override", pattern: /[\u202A-\u202E\u2066-\u2069]/, severity: "medium" },

  // Base64 encoded payloads (suspiciously long base64 in content)
  { name: "base64_payload", pattern: /[A-Za-z0-9+/]{100,}={0,2}/, severity: "low" },
];

// ─── Types ──────────────────────────────────────────────────────────────────

export interface InjectionScanResult {
  clean: boolean;
  threats: InjectionThreat[];
  sanitized: string;
}

export interface InjectionThreat {
  pattern: string;
  severity: "high" | "medium" | "low";
  match: string;
  position: number;
}

// ─── Scanning ───────────────────────────────────────────────────────────────

/**
 * Scan text for prompt injection patterns.
 * Returns threats found and a sanitized version with threats removed.
 */
export function scanForInjection(text: string): InjectionScanResult {
  const threats: InjectionThreat[] = [];
  let sanitized = text;

  for (const { name, pattern, severity } of INJECTION_PATTERNS) {
    const match = pattern.exec(text);
    if (match) {
      threats.push({
        pattern: name,
        severity,
        match: match[0].slice(0, 80),
        position: match.index,
      });

      // For high severity, strip the matched content
      if (severity === "high") {
        sanitized = sanitized.replace(pattern, "[REDACTED]");
      }
    }
  }

  // Strip zero-width characters regardless of pattern match
  sanitized = sanitized.replace(/[\u200B\u200C\u200D\uFEFF\u00AD]/g, "");

  return {
    clean: threats.length === 0,
    threats,
    sanitized,
  };
}

/**
 * Scan an array of memory entries and return only clean ones.
 * Logs warnings for any detected injection attempts.
 */
export function sanitizeMemoryEntries(entries: string[]): string[] {
  return entries.filter((entry) => {
    const result = scanForInjection(entry);
    if (!result.clean) {
      const highThreats = result.threats.filter((t) => t.severity === "high");
      if (highThreats.length > 0) {
        console.warn(
          `[injection-guard] Blocked memory entry with ${highThreats.length} high-severity threat(s): ` +
          highThreats.map((t) => t.pattern).join(", "),
        );
        return false; // Drop entries with high-severity threats
      }
      // Medium/low severity — keep but log
      console.log(
        `[injection-guard] Memory entry has ${result.threats.length} low/medium threat(s): ` +
        result.threats.map((t) => t.pattern).join(", "),
      );
    }
    return true;
  });
}

/**
 * Scan and sanitize a skill file's content before injecting into prompt.
 */
export function sanitizeSkillContent(content: string): string {
  const result = scanForInjection(content);
  if (!result.clean) {
    const highCount = result.threats.filter((t) => t.severity === "high").length;
    if (highCount > 0) {
      console.warn(
        `[injection-guard] Skill content has ${highCount} high-severity injection(s), using sanitized version`,
      );
      return result.sanitized;
    }
  }
  return content;
}
