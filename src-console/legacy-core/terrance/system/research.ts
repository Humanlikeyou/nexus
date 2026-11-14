/**
 * Amara Background Research
 *
 * Autonomous web research triggered by /research command or goals.
 * Searches web → reads top results → LLM synthesizes → stores as session log.
 */

import { invokeBound } from "../../../bridge";
import type { ResearchTask, ResearchResult, RouterResponse } from "../../lib/types";
import { getConfig } from "../../lib/api";
import { buildRouterConfig, tierSource } from "../../lib/llm-router";
import { activityBus } from "./activityBus";
import { jobFailureReason } from "./continuity";

export async function runResearchTask(
  topic: string,
  context?: string,
): Promise<ResearchTask> {
  const task: ResearchTask = {
    id: `research-${Date.now()}`,
    topic,
    status: "pending",
    query: topic,
    results: [],
    summary: null,
    timestamp: Date.now(),
  };

  const busId = activityBus.register(`Research: ${topic.slice(0, 40)}`, "research");

  try {
    // Step 1: Search the web
    task.status = "searching";
    activityBus.update(busId, "running", "Searching web...", 20);
    const searchResults = await invokeBound<ResearchResult[]>("web_search", {
      query: topic,
      maxResults: 5,
    });
    task.results = searchResults;

    if (searchResults.length === 0) {
      task.status = "error";
      task.summary = "No search results found for this topic.";
      return task;
    }

    // Step 2: Fetch content from top 3 results
    task.status = "reading";
    activityBus.update(busId, "running", `Reading ${Math.min(3, searchResults.length)} pages...`, 40);
    const pageContents: string[] = [];
    const urlsToRead = searchResults.slice(0, 3);

    for (const result of urlsToRead) {
      try {
        const content = await invokeBound<string>("fetch_url", {
          url: result.url,
          maxChars: 5000,
        });
        pageContents.push(`## ${result.title}\nSource: ${result.url}\n\n${content}`);
      } catch {
        pageContents.push(`## ${result.title}\nSource: ${result.url}\n\n[Failed to fetch content]\nSnippet: ${result.snippet}`);
      }
    }

    // Step 3: Synthesize via LLM
    task.status = "synthesizing";
    activityBus.update(busId, "running", "Synthesizing findings...", 70);
    const synthesisPrompt =
      `You are Amara, conducting research on: "${topic}"` +
      (context ? `\n\nContext: ${context}` : "") +
      `\n\nHere are the web sources I found:\n\n${pageContents.join("\n\n---\n\n")}` +
      `\n\nSynthesize a clear, actionable research summary:\n` +
      `1. Key findings (bullet points)\n` +
      `2. Most relevant sources with brief annotations\n` +
      `3. Recommendations or next steps\n` +
      `4. Confidence level and gaps in the research\n\n` +
      `Be concise and factual. Cite sources by name.`;

    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    const resp = await invokeBound<RouterResponse>("chat_with_router", {
      text: synthesisPrompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
    });

    task.summary = resp.content.trim();
    task.status = "complete";
    activityBus.complete(busId, `${searchResults.length} sources synthesized`);

    // Store as session log
    await invokeBound<void>("store_session_log", {
      entry: {
        id: task.id,
        timestamp: Date.now(),
        messageCount: 0,
        summary: `[Research: ${topic}]\n${task.summary}`,
        ...tierSource(resp),
      },
    });

    return task;
  } catch (e) {
    task.status = "error";
    console.error("[research] failed:", e);
    task.summary = `Research failed: ${jobFailureReason(e)}`;
    activityBus.fail(busId, jobFailureReason(e));
    return task;
  }
}
