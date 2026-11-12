import { explain } from "./act";
import { useRef, useState } from "react";
import { api } from "./api";
import type { DrillVoiceReply } from "./api";
import { keepDates, withCode } from "./text";
import { useData } from "./useData";
import { Section, State } from "./views";

// Voice practice (#56): the browser reads the question aloud (speechSynthesis, on this device),
// records the answer (MediaRecorder), and the careerbrew server transcribes it locally and grades it.
// The typed box is the fallback when there is no microphone or no speech-to-text engine.

function toBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result).split(",", 2)[1] ?? "");
    r.onerror = () => reject(r.error ?? new Error("could not read the recording"));
    r.readAsDataURL(blob);
  });
}

export function VoicePractice() {
  const q = useData(api.drill);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reply, setReply] = useState<DrillVoiceReply | null>(null);
  const rec = useRef<MediaRecorder | null>(null);
  const canSpeak = typeof window !== "undefined" && "speechSynthesis" in window;
  const canRecord = typeof navigator !== "undefined" && !!navigator.mediaDevices?.getUserMedia && typeof MediaRecorder !== "undefined";

  const grade = async (item: string, body: { text?: string; audio?: string; mime?: string }) => {
    setBusy(true); setError(null);
    try { setReply(await api.drillVoice({ item, ...body })); setText(""); }
    catch (e) { setError(explain(e)); }
    finally { setBusy(false); }
  };

  const toggleRecord = async (item: string) => {
    if (recording && rec.current) { rec.current.stop(); return; }
    setError(null);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const r = new MediaRecorder(stream);
      const chunks: Blob[] = [];
      r.ondataavailable = (e) => chunks.push(e.data);
      r.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop());
        setRecording(false);
        const blob = new Blob(chunks, { type: r.mimeType || "audio/webm" });
        await grade(item, { audio: await toBase64(blob), mime: blob.type });
      };
      rec.current = r; r.start(); setRecording(true);
    } catch (e) { setError(`Microphone not available (${e instanceof Error ? e.message : String(e)}). Type your answer instead.`); }
  };

  return (
    <Section title="Practice by voice" block="voice">
      <State q={q} label="a practice question">
        {(d) => !d.item ? <p className="empty">Nothing due. Come back when reviews are due.</p> : (
          <div className="stack">
            <p><span className="tag">{d.item.topic}</span> {d.item.prompt}</p>
            <ul className="list small" aria-label="Rubric">{d.criteria.map((c) => <li key={c.id}>{c.text}</li>)}</ul>
            <p className="chips">
              {canSpeak && <button type="button" onClick={() => { window.speechSynthesis.cancel(); window.speechSynthesis.speak(new SpeechSynthesisUtterance(d.item!.prompt)); }}>Read question aloud</button>}
              {d.stt && canRecord && <button type="button" disabled={busy} onClick={() => void toggleRecord(d.item!.id)}>{recording ? "Stop and grade" : "Record answer"}</button>}
            </p>
            {!d.stt && <p className="small muted">Speech-to-text is not set up. It runs only on this machine: install whisper.cpp, then run <code>careerbrew config set stt_model &lt;model file&gt;</code>. Until then, type your answer.</p>}
            <form onSubmit={(e) => { e.preventDefault(); if (text.trim()) void grade(d.item!.id, { text }); }}>
              <textarea aria-label="Typed answer" rows={4} value={text} onChange={(e) => setText(e.target.value)} placeholder="Or type your answer" />
              <button type="submit" disabled={busy || !text.trim()}>Grade typed answer</button>
              {busy && <span className="muted small working" aria-live="polite">Grading…</span>}
            </form>
            {error && <p className="callout callout-error" role="alert">{withCode(error)}</p>}
            {reply && (
              <div aria-label="Voice result">
                {reply.source === "audio" && <p className="small"><em>You said:</em> {reply.transcript}</p>}
                <p>Met: {reply.row.met.join(", ") || "none"}</p>
                <ul className="list small">{reply.feedback.map((l) => <li key={l}>{keepDates(l)}</li>)}</ul>
                <button type="button" onClick={() => { setReply(null); q.reload(); }}>Next question</button>
              </div>
            )}
          </div>
        )}
      </State>
    </Section>
  );
}
