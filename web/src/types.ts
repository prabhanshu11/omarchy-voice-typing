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
