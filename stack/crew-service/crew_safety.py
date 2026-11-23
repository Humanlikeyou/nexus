"""Shared redaction for the crew-service modules that handle multi-line text (verification output,
shell output, summaries, audit targets). main.sanitize_text keeps the first line only; these keep
the shape of the text but never a secret-looking token or URL credential.
"""

from __future__ import annotations

import re
from typing import Optional

_URL_CREDS = re.compile(r"(\w+://)[^/\s:@]+:[^/\s@]+@")
_SECRETISH = re.compile(
    r"(?i)(bearer\s+\S+|(?:api[_-]?key|token|password|secret|passwd|authorization)\s*[=:]\s*\S+|"
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----|[A-Za-z0-9_\-]{32,})")


def redact_line(line: str) -> str:
    line = _URL_CREDS.sub(r"\1***@", line)
    line = _SECRETISH.sub("[redacted]", line)
    return "".join(ch for ch in line if ch.isprintable() or ch == "\t")


def redact_block(text: Optional[str], max_chars: int = 4000, keep: str = "tail") -> str:
    """Redact every line, drop control characters, cap to max_chars (keeping the tail by default:
    the end of a test run is where the failure is)."""
    if not text:
        return ""
    out = "\n".join(redact_line(ln) for ln in str(text).splitlines())
    if len(out) > max_chars:
        out = ("…" + out[-max_chars:]) if keep == "tail" else (out[:max_chars] + "…")
    return out


def redact_one_line(text: Optional[str], max_chars: int = 200) -> str:
    """One line, redacted and capped. For labels, targets and short fields."""
    flat = " ".join(str(text or "").split())
    out = redact_line(flat)
    return out if len(out) <= max_chars else out[:max_chars] + "…"
