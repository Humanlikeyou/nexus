import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

const getAllowClaude = vi.fn();
const setAllowClaude = vi.fn();
vi.mock("../bridge", () => ({ getAllowClaude: () => getAllowClaude(), setAllowClaude: (v: boolean) => setAllowClaude(v) }));
import { ALLOW_CLAUDE_COPY, AllowClaudeGroup } from "./AllowClaude";

const toggle = () => screen.getByRole("switch", { name: "Allow Claude" });

describe("AllowClaudeGroup", () => {
  beforeEach(() => { getAllowClaude.mockReset(); setAllowClaude.mockReset(); });

  it("is off by default and says data leaves the Mac", async () => {
    getAllowClaude.mockResolvedValue(false);
    render(<AllowClaudeGroup />);
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    expect(toggle()).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(ALLOW_CLAUDE_COPY.subtitle)).toBeInTheDocument();
    expect(ALLOW_CLAUDE_COPY.subtitle).toMatch(/off the Mac/);
  });

  it("stays off and disabled until the saved value loads; a failed read stays off", async () => {
    getAllowClaude.mockRejectedValue(new Error("x"));
    render(<AllowClaudeGroup />);
    expect(toggle()).toBeDisabled();
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    expect(toggle()).toHaveAttribute("aria-checked", "false");
  });

  it("shows the saved value and saves a change", async () => {
    getAllowClaude.mockResolvedValue(true);
    setAllowClaude.mockResolvedValue(false);
    render(<AllowClaudeGroup />);
    await waitFor(() => expect(toggle()).toHaveAttribute("aria-checked", "true"));
    fireEvent.click(toggle());
    expect(setAllowClaude).toHaveBeenCalledWith(false);
    await waitFor(() => expect(toggle()).toHaveAttribute("aria-checked", "false"));
  });

  it("reverts and shows fixed copy when saving fails", async () => {
    getAllowClaude.mockResolvedValue(false);
    setAllowClaude.mockRejectedValue(new Error("disk full at /secret/path"));
    render(<AllowClaudeGroup />);
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    fireEvent.click(toggle());
    await waitFor(() => expect(screen.getByText(ALLOW_CLAUDE_COPY.error)).toBeInTheDocument());
    expect(toggle()).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByText(/secret/)).toBeNull();
  });
});
