// voice messages go as files, directly, never through the mailbox
import { reactive } from "vue";

interface Candidate {
  type: string;
  mime: string;
  extension: string;
}

// AAC in MP4 plays on both, and iOS records nothing else
const CANDIDATES: Candidate[] = [
  { type: "audio/mp4;codecs=mp4a.40.2", mime: "audio/mp4", extension: "m4a" },
  { type: "audio/mp4", mime: "audio/mp4", extension: "m4a" },
  { type: "audio/webm;codecs=opus", mime: "audio/webm", extension: "webm" },
];

export const media = {
  getUserMedia: (constraints: MediaStreamConstraints) => navigator.mediaDevices.getUserMedia(constraints),
  createRecorder: (stream: MediaStream, options: { mimeType: string }) => new MediaRecorder(stream, options),
  isTypeSupported: (type: string) => typeof MediaRecorder !== "undefined" && MediaRecorder.isTypeSupported(type),
};

export const recording = reactive({ active: false, startedAt: 0 });

let stream: MediaStream | null = null;
let recorder: MediaRecorder | null = null;
let chunks: Blob[] = [];
let format: Candidate | null = null;

function choose(isSupported: (type: string) => boolean): Candidate | null {
  return CANDIDATES.find((candidate) => isSupported(candidate.type)) ?? null;
}

export function pickFormat(isSupported: (type: string) => boolean): { mime: string; extension: string } | null {
  const candidate = choose(isSupported);
  return candidate && { mime: candidate.mime, extension: candidate.extension };
}

function release() {
  stream?.getTracks().forEach((track) => track.stop());
  stream = null;
  recorder = null;
  recording.active = false;
}

export type RecordingStart = "recording" | "unsupported" | "denied";

export async function startRecording(): Promise<RecordingStart> {
  format = choose(media.isTypeSupported);
  if (!format) return "unsupported";
  try {
    stream = await media.getUserMedia({ audio: true });
  } catch {
    return "denied";
  }
  chunks = [];
  recorder = media.createRecorder(stream, { mimeType: format.type });
  recorder.ondataavailable = (event) => chunks.push(event.data);
  recorder.start();
  Object.assign(recording, { active: true, startedAt: Date.now() });
  return "recording";
}

const pad = (value: number) => String(value).padStart(2, "0");

function stamp(date: Date): string {
  const day = `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}`;
  return `${day}-${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`;
}

export async function stopRecording(): Promise<File | null> {
  const active = recorder;
  const chosen = format;
  if (!active || !chosen) return null;
  const stopped = new Promise<void>((resolve) => (active.onstop = () => resolve()));
  active.stop();
  await stopped;
  release();
  return new File(chunks, `voice-${stamp(new Date())}.${chosen.extension}`, { type: chosen.mime });
}

export function cancelRecording() {
  if (recorder && recorder.state !== "inactive") {
    recorder.onstop = null;
    recorder.stop();
  }
  chunks = [];
  release();
}
