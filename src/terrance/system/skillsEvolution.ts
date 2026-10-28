/**
 * Skills Self-Improvement Loop
 *
 * Amara autonomously detects patterns in session logs and creates
 * or updates reusable skill files that get loaded into future prompts.
 *
 * Inspired by Hermes Agent's skills_tool.py.
 *
 * Flow:
 *  1. Analyze recent session logs for repeated patterns/topics
 *  2. If a pattern appears 3+ times, propose a new skill
 *  3. Generate skill content via LLM
 *  4. Save to ~/.openclaw/skills/ as a .md file
 *  5. On subsequent runs, check if existing skills need updating
 *
 * Runs on the consolidation schedule (every 6 hours).
 */

import { invoke } from "@tauri-apps/api/core";
import { getConfig } from "@/lib/api";
import { buildRouterConfig } from "@/lib/llm-router";
import { invalidateSkillsCache } from "./continuity";
import type { RouterResponse, SessionLogEntry } from "@/lib/types";
import { activityBus } from "./activityBus";

// ─── Types ──────────────────────────────────────────────────────────────────

interface SkillProposal {
  name: string;
  description: string;
  reason: string;
  isUpdate: boolean;
}

interface ExistingSkill {
  name: string;
  status: string;
  description: string;
  content: string;
}

// ─── Constants ──────────────────────────────────────────────────────────────

/** Minimum occurrences of a topic pattern to warrant a skill */
const MIN_PATTERN_COUNT = 3;

/** Maximum skills to create per evolution run */
const MAX_NEW_SKILLS_PER_RUN = 2;

/** Cooldown between evolution runs (6 hours, aligned with consolidation) */
const EVOLUTION_COOLDOWN_MS = 6 * 60 * 60 * 1000;

let _lastEvolutionRun = 0;

// ─── Core ───────────────────────────────────────────────────────────────────

/**
 * Main entry point: analyze recent sessions and evolve skills.
 * Called from the scheduler on the consolidation tick.
 */
export async function evolveSkills(): Promise<{
  proposed: number;
  created: number;
  updated: number;
}> {
  const now = Date.now();
  if (now - _lastEvolutionRun < EVOLUTION_COOLDOWN_MS) {
    return { proposed: 0, created: 0, updated: 0 };
  }
  _lastEvolutionRun = now;

  try {
    // Load recent session logs (last 7 days)
    const allLogs = await invoke<SessionLogEntry[]>("list_session_logs");
    const cutoff = now - 7 * 24 * 60 * 60 * 1000;
    const recent = allLogs.filter((l) => l.timestamp >= cutoff);

    if (recent.length < MIN_PATTERN_COUNT) {
      return { proposed: 0, created: 0, updated: 0 };
    }

    // Load existing skills
    const existingSkills = await invoke<ExistingSkill[]>("read_skills");
    const existingNames = new Set(existingSkills.map((s) => s.name.toLowerCase()));

    // Ask LLM to identify patterns and propose skills
    const proposals = await identifyPatterns(recent, existingSkills);
    if (!proposals || proposals.length === 0) {
      return { proposed: 0, created: 0, updated: 0 };
    }

    let created = 0;
    let updated = 0;

    for (const proposal of proposals.slice(0, MAX_NEW_SKILLS_PER_RUN)) {
      const isExisting = existingNames.has(proposal.name.toLowerCase());

      if (isExisting && proposal.isUpdate) {
        // Update existing skill
        const content = await generateSkillContent(proposal, existingSkills);
        if (content) {
          await invoke<void>("save_skill", { name: proposal.name, content });
          updated++;
          console.log(`[skills-evolution] Updated skill: ${proposal.name}`);
        }
      } else if (!isExisting) {
        // Create new skill
        const content = await generateSkillContent(proposal, []);
        if (content) {
          await invoke<void>("save_skill", { name: proposal.name, content });
          created++;
          console.log(`[skills-evolution] Created skill: ${proposal.name}`);
        }
      }
    }

    // Invalidate skills cache so new skills load on next prompt build
    if (created > 0 || updated > 0) {
      invalidateSkillsCache();
    }

    return { proposed: proposals.length, created, updated };
  } catch (e) {
    console.error("[skills-evolution] Error:", e);
    return { proposed: 0, created: 0, updated: 0 };
  }
}

// ─── Pattern Identification ─────────────────────────────────────────────────

async function identifyPatterns(
  logs: SessionLogEntry[],
  existingSkills: ExistingSkill[],
): Promise<SkillProposal[] | null> {
  const summaries = logs
    .slice(0, 20) // Cap to avoid huge prompts
    .map((l, i) => `${i + 1}. [${new Date(l.timestamp).toLocaleDateString()}] ${l.summary}`)
    .join("\n");

  const existingList = existingSkills.length > 0
    ? existingSkills.map((s) => `- ${s.name}: ${s.description}`).join("\n")
    : "None";

  const prompt =
    `You are Amara's skill evolution engine. Analyze these recent session summaries ` +
    `and identify repeating patterns, topics, or workflows that appear 3+ times.\n\n` +
    `Existing skills:\n${existingList}\n\n` +
    `Recent sessions:\n${summaries}\n\n` +
    `For each pattern found, output a JSON array of skill proposals:\n` +
    `[{"name": "skill-name", "description": "one line", "reason": "why this should be a skill", "isUpdate": false}]\n\n` +
    `Rules:\n` +
    `- Only propose skills for genuinely repeated patterns (3+ occurrences)\n` +
    `- Don't duplicate existing skills unless they need meaningful updates\n` +
    `- Set isUpdate=true only if an existing skill needs new information\n` +
    `- Skill names should be lowercase-hyphenated (e.g. "goal-tracking")\n` +
    `- Return empty array [] if no patterns warrant new skills\n` +
    `- Output ONLY the JSON array, no other text`;

  try {
    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const resp = await invoke<RouterResponse>("chat_with_router", {
      text: prompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: cfg.anthropic_api_key ?? undefined,
      gatewayUrl: cfg.gateway_url ?? undefined,
      gatewayAuthToken: cfg.gateway_auth_token ?? undefined,
    });

    // Parse JSON from response
    const content = resp.content.trim();
    const jsonMatch = content.match(/\[[\s\S]*\]/);
    if (!jsonMatch) return [];
    return JSON.parse(jsonMatch[0]) as SkillProposal[];
  } catch (e) {
    console.error("[skills-evolution] Pattern identification failed:", e);
    return null;
  }
}

// ─── Skill Content Generation ───────────────────────────────────────────────

async function generateSkillContent(
  proposal: SkillProposal,
  existingSkills: ExistingSkill[],
): Promise<string | null> {
  const existingContent = existingSkills
    .find((s) => s.name.toLowerCase() === proposal.name.toLowerCase())
    ?.content;

  const updateNote = existingContent
    ? `\n\nExisting skill content to update/merge:\n${existingContent}`
    : "";

  const prompt =
    `Generate a skill file for Amara (a personal AI assistant). ` +
    `This skill will be injected into Amara's system prompt when active.\n\n` +
    `Skill: ${proposal.name}\n` +
    `Description: ${proposal.description}\n` +
    `Reason: ${proposal.reason}\n${updateNote}\n\n` +
    `Format the skill as a Markdown document with:\n` +
    `# ${proposal.name}\n` +
    `> One-line description\n\n` +
    `Then sections for:\n` +
    `- **When to use**: Conditions that trigger this skill\n` +
    `- **Approach**: Step-by-step how to handle this pattern\n` +
    `- **Key details**: Specific values, paths, or preferences to remember\n` +
    `- **Mistakes to avoid**: Common pitfalls\n\n` +
    `Keep it concise (under 300 words). Make it actionable.`;

  try {
    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const resp = await invoke<RouterResponse>("chat_with_router", {
      text: prompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: cfg.anthropic_api_key ?? undefined,
      gatewayUrl: cfg.gateway_url ?? undefined,
      gatewayAuthToken: cfg.gateway_auth_token ?? undefined,
    });

    return resp.content.trim();
  } catch (e) {
    console.error("[skills-evolution] Content generation failed:", e);
    return null;
  }
}
