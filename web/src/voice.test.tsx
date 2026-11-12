import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { VoicePractice } from "./voice";

afterEach(() => vi.unstubAllGlobals());

describe("VoicePractice", () => {
  it("without speech-to-text, names the local setup and grades a typed answer", async () => {
    const fetchMock = vi.fn((url: string) => Promise.resolve(new Response(JSON.stringify(String(url).endsWith("/api/drill/voice")
      ? { transcript: "I decided", source: "typed", row: { met: ["ownership"], missed: ["result"], score: 0.2, rating: "again" }, feedback: ["Practice: Gave a measurable result"] }
      : { item: { id: "beh-impact-1", topic: "behavioral", prompt: "Tell me about your proudest project.", mode: "learn" }, criteria: [{ id: "ownership", text: "Focused on own actions" }], stt: false }))));
    vi.stubGlobal("fetch", fetchMock);
    render(<VoicePractice />);
    expect(await screen.findByText(/proudest project/)).toBeInTheDocument();
    expect(screen.getByText(/Speech-to-text is not set up/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Record answer" })).toBeNull();
    fireEvent.change(screen.getByLabelText("Typed answer"), { target: { value: "I decided" } });
    fireEvent.click(screen.getByRole("button", { name: "Grade typed answer" }));
    expect(await screen.findByText("Met: ownership")).toBeInTheDocument();
    expect(screen.getByText("Practice: Gave a measurable result")).toBeInTheDocument();
    expect(screen.queryByText(/You said/)).toBeNull();
    const body = JSON.parse(String((fetchMock.mock.calls.at(-1) as unknown as [string, RequestInit])[1].body));
    expect(body).toEqual({ item: "beh-impact-1", text: "I decided" });
  });
});
