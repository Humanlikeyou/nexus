# Lessons learned (verified only)

Each entry: date, what happened, verified cause, the rule now followed. Unverified hunches don't go here.

## 2026-11-11: Unquoted heredocs run backticks
- **What:** writing agent files with `cat > f <<E` (unquoted) and backticked commands in the text made the shell execute `npx tsc`, `npm run lint` and `npm run test:unit` while writing the file, and left blanks in the file.
- **Cause (verified):** in an unquoted heredoc the shell performs command substitution on `` `...` ``.
- **Rule:** always quote the delimiter (`<<'EOF'`) when writing docs or config from the shell, or use the Write tool.
