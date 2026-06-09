# rust-js-python-reviewer-pro (v2.1)

**Production-grade independent code reviewer** for **Rust + JavaScript/TypeScript/HTML + Python + Shell/SystemD** — the exact language mix used in ApexOS.

Designed for objective **fresh-eyes** reviews where the reviewer has **zero prior involvement**. The skill enforces project locked decisions (prevents wild-goose-chase PRs) while guiding reviewers to find real bugs, correctness issues, and long-term maintenance problems.

## Files

- `SKILL.md` — The complete reusable base skill. Load this when reviewing any polyglot Rust+JS+Python+Shell project.
- `review-prompt.md` — Ready-to-paste generic reviewer prompt (fill in the bracketed fields).
- `apexos-reviewer.md` — **ApexOS-specific reviewer prompt** (use this for reviewing this repo). Pre-loads current architecture, locked decisions table, and high-priority focus areas. This is the recommended entry point.

## Quick Usage (ApexOS)

Paste the ApexOS-specific prompt into a fresh Claude session:

```bash
# Linux
cat docs/claude/skills/rust-js-python-reviewer-pro/apexos-reviewer.md | xclip -selection clipboard

# macOS
cat docs/claude/skills/rust-js-python-reviewer-pro/apexos-reviewer.md | pbcopy
```

Or open the file and paste its contents manually.

For maximum independence on large reviews, dispatch multiple specialist sub-agents (Security, Maintainability, Architecture) each given a narrowed version of the prompt + the skill, then synthesize in the main session.

## What This Skill Prevents

- "Why not use RPi.GPIO?" — broken on Pi 5 RP1, locked out
- "Add React/webpack" — no bundler/CDN is a locked decision
- "Just use tower ServeDir" — breaks under ProtectSystem=strict, locked out
- "Rewrite in Go/Python" — Rust single binary is locked
- Generic "add more tests" without pointing to a specific gap with file:line evidence

## What This Skill Produces

- Real findings with file:line + evidence from actual command output
- Correctness issues in self-evolution, policy engine, event bus, hardware tools
- Long-term maintainability signals (ownership, error paths, edge cases)
- Prioritized, actionable next steps with no noise

## Adapting for Other Projects

To use `SKILL.md` on a non-ApexOS codebase, use `review-prompt.md` as your template.  
Fill the bracketed sections with your repo path, locked decisions, and focus areas.  
The locked-decisions concept is the most valuable part — always extract them from the project's CLAUDE.md or equivalent before reviewing.
