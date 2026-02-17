package handlers

import (
	"encoding/json"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"sync"
	"time"
)

// LatencyMetrics holds all timing information for a transcription request
type LatencyMetrics struct {
	// Request identification
	RequestID   string    `json:"request_id"`
	Timestamp   time.Time `json:"timestamp"`

	// Client-provided timing (when recording started/ended)
	RecordStartTime *time.Time `json:"record_start_time,omitempty"`
	RecordEndTime   *time.Time `json:"record_end_time,omitempty"`

	// Server-side timing
	RequestReceivedTime time.Time `json:"request_received_time"`

	// Upload to AssemblyAI timing
	UploadStartTime *time.Time `json:"upload_start_time,omitempty"`
	UploadEndTime   *time.Time `json:"upload_end_time,omitempty"`

	// Transcription job timing
	TranscriptionStartTime *time.Time `json:"transcription_start_time,omitempty"`
	TranscriptionEndTime   *time.Time `json:"transcription_end_time,omitempty"`

	// Text output timing (for future streaming support)
	TextStartTime *time.Time `json:"text_start_time,omitempty"`
	TextEndTime   *time.Time `json:"text_end_time,omitempty"`

	// Response sent timing
	TextOutTime *time.Time `json:"text_out_time,omitempty"`

	// Calculated latencies (in milliseconds)
	TotalLatencyMs        *int64 `json:"total_latency_ms,omitempty"`
	UploadMs              *int64 `json:"upload_ms,omitempty"`
	TranscriptionMs       *int64 `json:"transcription_ms,omitempty"`
	RecordingDurationMs   *int64 `json:"recording_duration_ms,omitempty"`
	EndToEndMs            *int64 `json:"end_to_end_ms,omitempty"` // from record_end to text_out

	// Additional metadata
	AudioDurationS     float64 `json:"audio_duration_s,omitempty"`
	TranscriptID       string  `json:"transcript_id,omitempty"`
	TranscriptLength   int     `json:"transcript_length,omitempty"`
	Success            bool    `json:"success"`
	ErrorMessage       string  `json:"error_message,omitempty"`
}

// NewLatencyMetrics creates a new LatencyMetrics with request ID and timestamp
func NewLatencyMetrics() *LatencyMetrics {
	now := time.Now()
	return &LatencyMetrics{
		RequestID:           fmt.Sprintf("%d", now.UnixNano()),
		Timestamp:           now,
		RequestReceivedTime: now,
	}
}

// SetRecordStartTime sets the recording start time from client header
func (lm *LatencyMetrics) SetRecordStartTime(t time.Time) {
	lm.RecordStartTime = &t
}

// SetRecordEndTime sets the recording end time from client header
func (lm *LatencyMetrics) SetRecordEndTime(t time.Time) {
	lm.RecordEndTime = &t
}

// MarkUploadStart records when upload to AssemblyAI started
func (lm *LatencyMetrics) MarkUploadStart() {
	now := time.Now()
	lm.UploadStartTime = &now
}

// MarkUploadEnd records when upload to AssemblyAI completed
func (lm *LatencyMetrics) MarkUploadEnd() {
	now := time.Now()
	lm.UploadEndTime = &now
}

// MarkTranscriptionStart records when transcription job was submitted
func (lm *LatencyMetrics) MarkTranscriptionStart() {
	now := time.Now()
	lm.TranscriptionStartTime = &now
}

// MarkTranscriptionEnd records when transcription completed
func (lm *LatencyMetrics) MarkTranscriptionEnd() {
	now := time.Now()
	lm.TranscriptionEndTime = &now
}

// MarkTextOut records when text was returned to client
// In batch mode, TextStartTime and TextEndTime are the same
func (lm *LatencyMetrics) MarkTextOut() {
	now := time.Now()
	lm.TextStartTime = &now
	lm.TextEndTime = &now
	lm.TextOutTime = &now
}

// CalculateLatencies computes all derived latency values
func (lm *LatencyMetrics) CalculateLatencies() {
	// Upload latency
	if lm.UploadStartTime != nil && lm.UploadEndTime != nil {
		ms := lm.UploadEndTime.Sub(*lm.UploadStartTime).Milliseconds()
		lm.UploadMs = &ms
	}

	// Transcription latency
	if lm.TranscriptionStartTime != nil && lm.TranscriptionEndTime != nil {
		ms := lm.TranscriptionEndTime.Sub(*lm.TranscriptionStartTime).Milliseconds()
		lm.TranscriptionMs = &ms
	}

	// Total latency (from request received to text out)
	if lm.TextOutTime != nil {
		ms := lm.TextOutTime.Sub(lm.RequestReceivedTime).Milliseconds()
		lm.TotalLatencyMs = &ms
	}

	// Recording duration (client-side)
	if lm.RecordStartTime != nil && lm.RecordEndTime != nil {
		ms := lm.RecordEndTime.Sub(*lm.RecordStartTime).Milliseconds()
		lm.RecordingDurationMs = &ms
	}

	// End-to-end latency (from recording end to text out)
	if lm.RecordEndTime != nil && lm.TextOutTime != nil {
		ms := lm.TextOutTime.Sub(*lm.RecordEndTime).Milliseconds()
		lm.EndToEndMs = &ms
	}
}

// LatencyResponse contains latency info to include in API response
type LatencyResponse struct {
	TotalLatencyMs   int64 `json:"total_latency_ms"`
	UploadMs         int64 `json:"upload_ms"`
	TranscriptionMs  int64 `json:"transcription_ms"`
	EndToEndMs       int64 `json:"end_to_end_ms,omitempty"`
}

// ToResponse creates a LatencyResponse for the API response
func (lm *LatencyMetrics) ToResponse() LatencyResponse {
	resp := LatencyResponse{}
	if lm.TotalLatencyMs != nil {
		resp.TotalLatencyMs = *lm.TotalLatencyMs
	}
	if lm.UploadMs != nil {
		resp.UploadMs = *lm.UploadMs
	}
	if lm.TranscriptionMs != nil {
		resp.TranscriptionMs = *lm.TranscriptionMs
	}
	if lm.EndToEndMs != nil {
		resp.EndToEndMs = *lm.EndToEndMs
	}
	return resp
}

// LatencyLogger handles writing latency metrics to JSONL file
type LatencyLogger struct {
	logDir  string
	logFile *os.File
	mu      sync.Mutex
}

var (
	defaultLatencyLogger *LatencyLogger
	loggerOnce           sync.Once
)

// GetLatencyLogger returns the singleton latency logger
func GetLatencyLogger() *LatencyLogger {
	loggerOnce.Do(func() {
		// Default log directory relative to gateway
		logDir := "../experiments/logs"
		if envDir := os.Getenv("LATENCY_LOG_DIR"); envDir != "" {
			logDir = envDir
		}

		logger, err := NewLatencyLogger(logDir)
		if err != nil {
			log.Printf("Warning: Failed to create latency logger: %v", err)
			// Create a no-op logger
			defaultLatencyLogger = &LatencyLogger{}
		} else {
			defaultLatencyLogger = logger
		}
	})
	return defaultLatencyLogger
}

// NewLatencyLogger creates a new latency logger
func NewLatencyLogger(logDir string) (*LatencyLogger, error) {
	// Create directory if it doesn't exist
	if err := os.MkdirAll(logDir, 0755); err != nil {
		return nil, fmt.Errorf("failed to create log directory: %w", err)
	}

	// Create log file with date-based naming
	logFileName := fmt.Sprintf("latency_%s.jsonl", time.Now().Format("2006-01-02"))
	logPath := filepath.Join(logDir, logFileName)

	file, err := os.OpenFile(logPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return nil, fmt.Errorf("failed to open log file: %w", err)
	}

	log.Printf("Latency logging to: %s", logPath)

	return &LatencyLogger{
		logDir:  logDir,
		logFile: file,
	}, nil
}

// Log writes a latency metrics entry to the JSONL file
func (ll *LatencyLogger) Log(metrics *LatencyMetrics) error {
	if ll.logFile == nil {
		return nil // No-op if logger not initialized
	}

	ll.mu.Lock()
	defer ll.mu.Unlock()

	// Check if we need to rotate to a new day's file
	currentDate := time.Now().Format("2006-01-02")
	expectedFileName := fmt.Sprintf("latency_%s.jsonl", currentDate)
	expectedPath := filepath.Join(ll.logDir, expectedFileName)

	// Get current file info
	if fi, err := ll.logFile.Stat(); err == nil {
		if fi.Name() != expectedFileName {
			// Need to rotate
			ll.logFile.Close()
			file, err := os.OpenFile(expectedPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
			if err != nil {
				return fmt.Errorf("failed to rotate log file: %w", err)
			}
			ll.logFile = file
			log.Printf("Rotated latency log to: %s", expectedPath)
		}
	}

	data, err := json.Marshal(metrics)
	if err != nil {
		return fmt.Errorf("failed to marshal metrics: %w", err)
	}

	if _, err := ll.logFile.Write(append(data, '\n')); err != nil {
		return fmt.Errorf("failed to write metrics: %w", err)
	}

	return nil
}

// Close closes the log file
func (ll *LatencyLogger) Close() error {
	if ll.logFile != nil {
		return ll.logFile.Close()
	}
	return nil
}

// ParseClientTimestamp parses timestamp from header or form field
// Accepts Unix milliseconds, Unix seconds, or RFC3339 format
func ParseClientTimestamp(value string) *time.Time {
	if value == "" {
		return nil
	}

	// Try Unix milliseconds (int64)
	var ms int64
	if _, err := fmt.Sscanf(value, "%d", &ms); err == nil {
		if ms > 1e12 {
			// Likely milliseconds
			t := time.UnixMilli(ms)
			return &t
		} else {
			// Likely seconds
			t := time.Unix(ms, 0)
			return &t
		}
	}

	// Try RFC3339
	if t, err := time.Parse(time.RFC3339, value); err == nil {
		return &t
	}

	// Try RFC3339Nano
	if t, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return &t
	}

	return nil
}
