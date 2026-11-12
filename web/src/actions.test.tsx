import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ActionsView } from "./actions";

const catalog = {
  actions: [
    { name: "config.set", command: "careerbrew config set", help: "validate and save a setting", ui: true, terminal_only: null, confirm: null,
      args: [{ name: "key", flag: null, kind: "text", choices: null, required: true, many: false, help: "", default: null }, { name: "value", flag: null, kind: "text", choices: null, required: true, many: false, help: "", default: null }] },
    { name: "machine.set", command: "careerbrew machine set", help: "set the role", ui: true, terminal_only: null, confirm: "changes this machine's role",
      args: [{ name: "role", flag: null, kind: "choice", choices: ["personal", "corporate"], required: true, many: false, help: "", default: null }] },
    { name: "secret.set", command: "careerbrew secret set", help: "hidden prompt", ui: false, terminal_only: "the secret goes to a hidden prompt", confirm: null, args: [] },
  ],
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("Actions view", () => {
  it("builds a form per action, runs it, asks to confirm, and points terminal-only actions to the terminal", async () => {
    const posts: { url: string; body: string }[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      if (init?.method === "POST") posts.push({ url: String(url), body: String(init.body) });
      const out = init?.method === "POST" ? { action: "config.set", argv: ["careerbrew", "config", "set", "work_end", "18:00"], exit: 0, output: "work_end = 18:00 (saved to settings.yaml)" } : catalog;
      return Promise.resolve(new Response(JSON.stringify(out), { status: 200 }));
    }));
    render(<ActionsView />);
    fireEvent.click(await screen.findByRole("button", { name: "config.set" }));
    fireEvent.change(screen.getByLabelText("key"), { target: { value: "work_end" } });
    fireEvent.change(screen.getByLabelText("value"), { target: { value: "18:00" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(await screen.findByText("work_end = 18:00 (saved to settings.yaml)")).toBeInTheDocument();
    expect(JSON.parse(posts[0].body)).toEqual({ values: { key: "work_end", value: "18:00" }, confirm: false });
    fireEvent.click(screen.getByRole("button", { name: "machine.set" }));
    expect(screen.getByRole("button", { name: "Run" })).toBeDisabled();
    fireEvent.click(screen.getByLabelText(/I understand/));
    expect(screen.getByRole("button", { name: "Run" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "secret.set" }));
    expect(screen.getByText(/Terminal only/)).toHaveTextContent("careerbrew secret set");
  });
});
