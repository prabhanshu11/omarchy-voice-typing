// ── Existing types (recordings) ──────────────────────

export interface Recording {
  filename: string;
  path: string;
  size: number;
  timestamp: string;
  duration?: number;
}

export interface Transcript {
  filename: string;
  path: string;
  size: number;
  timestamp: string;
  text?: string;
  recording_id?: string;
}

export interface LinkedEntry {
  recording?: Recording;
  transcript?: Transcript;
}

export interface Stats {
  total_recordings: number;
  total_transcripts: number;
  total_audio_size_mb: number;
  total_transcript_kb: number;
}

// ── Profiling types ──────────────────────────────────

export interface LatencyRecord {
  request_id: string;
  timestamp_unix: number;
  session_id: string;
  recording_number?: number;
  deepgram_connect_ms?: number;
  transcription_ms?: number;
  total_commit_ms?: number;
  upload_ms?: number;
  total_latency_ms?: number;
  end_to_end_ms?: number;
  audio_duration_s: number;
  audio_bytes?: number;
  backend: string;
  transcript_id: string;
  transcript_length: number;
  success: boolean;
  error_message: string;
}

export interface TimelineEvent {
  elapsed_s: number;
  layer: string;
  event: string;
}

export interface SessionSummary {
  filename: string;
  session_id: string;
  started: string;
  duration_s: number;
  status: string;
  backend: string;
  transcript: string;
  dg_connect_ms: number;
  first_audio_ms: number;
  transcribe_ms: number;
  total_ms: number;
  py_chunks: number;
  gw_bytes: number;
  timeline: TimelineEvent[];
}

export interface ProfilingSummary {
  total_sessions: number;
  success_count: number;
  failure_count: number;
  success_rate: number;
  total_commit_ms_p50?: number;
  total_commit_ms_p95?: number;
  total_commit_ms_p99?: number;
  transcription_ms_p50?: number;
  transcription_ms_p95?: number;
  transcription_ms_p99?: number;
  by_backend: BackendStats[];
  by_date: DailyCount[];
}

export interface BackendStats {
  backend: string;
  count: number;
  avg_total_ms: number;
  p50_total_ms: number;
  p95_total_ms: number;
}

export interface DailyCount {
  date: string;
  total: number;
  success: number;
  failure: number;
}
