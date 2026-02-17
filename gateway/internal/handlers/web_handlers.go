package handlers

import (
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

// Recording represents a single audio recording
type Recording struct {
	Filename  string    `json:"filename"`
	Path      string    `json:"path"`
	Size      int64     `json:"size"`
	Timestamp time.Time `json:"timestamp"`
	Duration  float64   `json:"duration,omitempty"` // in seconds, if known
}

// Transcript represents a single transcript
type Transcript struct {
	Filename    string    `json:"filename"`
	Path        string    `json:"path"`
	Size        int64     `json:"size"`
	Timestamp   time.Time `json:"timestamp"`
	Text        string    `json:"text,omitempty"`
	RecordingID string    `json:"recording_id,omitempty"`
}

// TranscriptUpdate is for editing transcripts
type TranscriptUpdate struct {
	Text string `json:"text"`
}

// Stats represents aggregated statistics
type Stats struct {
	TotalRecordings   int     `json:"total_recordings"`
	TotalTranscripts  int     `json:"total_transcripts"`
	TotalAudioSizeMB  float64 `json:"total_audio_size_mb"`
	TotalTranscriptKB float64 `json:"total_transcript_kb"`
}

// WebHandler holds configuration for web endpoints
type WebHandler struct {
	RecordingsDir  string
	TranscriptsDir string
}

// NewWebHandler creates a new web handler
func NewWebHandler(recordingsDir, transcriptsDir string) *WebHandler {
	return &WebHandler{
		RecordingsDir:  recordingsDir,
		TranscriptsDir: transcriptsDir,
	}
}

// parseTimestamp extracts timestamp from filename like 20260105_211652_audio.wav
func parseTimestamp(filename string) time.Time {
	parts := strings.Split(filename, "_")
	if len(parts) >= 2 {
		dateStr := parts[0] + parts[1]
		t, err := time.Parse("2006010215405", dateStr)
		if err == nil {
			return t
		}
		// Try with full format
		t, err = time.Parse("20060102150405", dateStr)
		if err == nil {
			return t
		}
	}
	return time.Time{}
}

// ListRecordingsHandler returns all recordings
func (wh *WebHandler) ListRecordingsHandler(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
		return
	}

	entries, err := os.ReadDir(wh.RecordingsDir)
	if err != nil {
		http.Error(w, "Failed to read recordings directory", http.StatusInternalServerError)
		return
	}

	var recordings []Recording
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		info, err := entry.Info()
		if err != nil {
			continue
		}
		name := entry.Name()
		if strings.HasSuffix(name, ".wav") || strings.HasSuffix(name, ".mp3") {
			recordings = append(recordings, Recording{
				Filename:  name,
				Path:      filepath.Join(wh.RecordingsDir, name),
				Size:      info.Size(),
				Timestamp: parseTimestamp(name),
			})
		}
	}

	// Sort by timestamp, newest first
	sort.Slice(recordings, func(i, j int) bool {
		return recordings[i].Timestamp.After(recordings[j].Timestamp)
	})

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(recordings)
}

// ListTranscriptsHandler returns all transcripts
func (wh *WebHandler) ListTranscriptsHandler(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
		return
	}

	entries, err := os.ReadDir(wh.TranscriptsDir)
	if err != nil {
		http.Error(w, "Failed to read transcripts directory", http.StatusInternalServerError)
		return
	}

	var transcripts []Transcript
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		info, err := entry.Info()
		if err != nil {
			continue
		}
		name := entry.Name()
		if strings.HasSuffix(name, ".txt") {
			transcripts = append(transcripts, Transcript{
				Filename:  name,
				Path:      filepath.Join(wh.TranscriptsDir, name),
				Size:      info.Size(),
				Timestamp: parseTimestamp(name),
			})
		}
	}

	// Sort by timestamp, newest first
	sort.Slice(transcripts, func(i, j int) bool {
		return transcripts[i].Timestamp.After(transcripts[j].Timestamp)
	})

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(transcripts)
}

// GetTranscriptHandler returns a single transcript's content
func (wh *WebHandler) GetTranscriptHandler(w http.ResponseWriter, r *http.Request) {
	filename := r.URL.Query().Get("filename")
	if filename == "" {
		http.Error(w, "filename parameter required", http.StatusBadRequest)
		return
	}

	// Sanitize filename to prevent directory traversal
	filename = filepath.Base(filename)
	path := filepath.Join(wh.TranscriptsDir, filename)

	switch r.Method {
	case http.MethodGet:
		content, err := os.ReadFile(path)
		if err != nil {
			if os.IsNotExist(err) {
				http.Error(w, "Transcript not found", http.StatusNotFound)
			} else {
				http.Error(w, "Failed to read transcript", http.StatusInternalServerError)
			}
			return
		}

		info, _ := os.Stat(path)
		transcript := Transcript{
			Filename:  filename,
			Path:      path,
			Size:      info.Size(),
			Timestamp: parseTimestamp(filename),
			Text:      string(content),
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(transcript)

	case http.MethodPut:
		var update TranscriptUpdate
		if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
			http.Error(w, "Invalid request body", http.StatusBadRequest)
			return
		}

		if err := os.WriteFile(path, []byte(update.Text), 0644); err != nil {
			http.Error(w, "Failed to save transcript", http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{"status": "saved"})

	default:
		http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
	}
}

// ServeAudioHandler serves audio files for playback
func (wh *WebHandler) ServeAudioHandler(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
		return
	}

	filename := r.URL.Query().Get("filename")
	if filename == "" {
		http.Error(w, "filename parameter required", http.StatusBadRequest)
		return
	}

	// Sanitize filename to prevent directory traversal
	filename = filepath.Base(filename)
	path := filepath.Join(wh.RecordingsDir, filename)

	file, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			http.Error(w, "Audio file not found", http.StatusNotFound)
		} else {
			http.Error(w, "Failed to open audio file", http.StatusInternalServerError)
		}
		return
	}
	defer file.Close()

	// Set content type based on extension
	if strings.HasSuffix(filename, ".mp3") {
		w.Header().Set("Content-Type", "audio/mpeg")
	} else {
		w.Header().Set("Content-Type", "audio/wav")
	}

	// Enable range requests for seeking
	info, _ := file.Stat()
	http.ServeContent(w, r, filename, info.ModTime(), file)
}

// StatsHandler returns aggregated statistics
func (wh *WebHandler) StatsHandler(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
		return
	}

	var stats Stats

	// Count recordings
	if entries, err := os.ReadDir(wh.RecordingsDir); err == nil {
		for _, entry := range entries {
			if !entry.IsDir() {
				name := entry.Name()
				if strings.HasSuffix(name, ".wav") || strings.HasSuffix(name, ".mp3") {
					stats.TotalRecordings++
					if info, err := entry.Info(); err == nil {
						stats.TotalAudioSizeMB += float64(info.Size()) / (1024 * 1024)
					}
				}
			}
		}
	}

	// Count transcripts
	if entries, err := os.ReadDir(wh.TranscriptsDir); err == nil {
		for _, entry := range entries {
			if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".txt") {
				stats.TotalTranscripts++
				if info, err := entry.Info(); err == nil {
					stats.TotalTranscriptKB += float64(info.Size()) / 1024
				}
			}
		}
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(stats)
}

// CORSMiddleware adds CORS headers for development
func CORSMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")

		if r.Method == "OPTIONS" {
			w.WriteHeader(http.StatusOK)
			return
		}

		next.ServeHTTP(w, r)
	})
}

// HealthHandler returns a simple health check
func HealthHandler(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}

// ServeStaticHandler serves static files from web/dist
func ServeStaticHandler(staticDir string) http.Handler {
	fs := http.FileServer(http.Dir(staticDir))
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Check if file exists
		path := filepath.Join(staticDir, r.URL.Path)
		if _, err := os.Stat(path); os.IsNotExist(err) {
			// Serve index.html for SPA routing
			http.ServeFile(w, r, filepath.Join(staticDir, "index.html"))
			return
		}
		fs.ServeHTTP(w, r)
	})
}

// ReadTranscriptText is a helper for the LinkRecordings endpoint
func (wh *WebHandler) ReadTranscriptText(filename string) string {
	content, err := os.ReadFile(filepath.Join(wh.TranscriptsDir, filename))
	if err != nil {
		return ""
	}
	return string(content)
}

// LinkedEntry represents a recording with its transcript
type LinkedEntry struct {
	Recording  *Recording  `json:"recording,omitempty"`
	Transcript *Transcript `json:"transcript,omitempty"`
}

// ListLinkedHandler returns recordings and transcripts linked by timestamp
func (wh *WebHandler) ListLinkedHandler(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
		return
	}

	// Get all recordings
	recEntries, _ := os.ReadDir(wh.RecordingsDir)
	recordings := make(map[string]*Recording)
	for _, entry := range recEntries {
		if entry.IsDir() {
			continue
		}
		name := entry.Name()
		if strings.HasSuffix(name, ".wav") || strings.HasSuffix(name, ".mp3") {
			info, _ := entry.Info()
			ts := parseTimestamp(name)
			key := ts.Format("20060102_1504")
			recordings[key] = &Recording{
				Filename:  name,
				Path:      filepath.Join(wh.RecordingsDir, name),
				Size:      info.Size(),
				Timestamp: ts,
			}
		}
	}

	// Get all transcripts
	txEntries, _ := os.ReadDir(wh.TranscriptsDir)
	transcripts := make(map[string]*Transcript)
	for _, entry := range txEntries {
		if entry.IsDir() {
			continue
		}
		name := entry.Name()
		if strings.HasSuffix(name, ".txt") {
			info, _ := entry.Info()
			ts := parseTimestamp(name)
			key := ts.Format("20060102_1504")
			content, _ := os.ReadFile(filepath.Join(wh.TranscriptsDir, name))
			transcripts[key] = &Transcript{
				Filename:  name,
				Path:      filepath.Join(wh.TranscriptsDir, name),
				Size:      info.Size(),
				Timestamp: ts,
				Text:      string(content),
			}
		}
	}

	// Merge into linked entries
	allKeys := make(map[string]bool)
	for k := range recordings {
		allKeys[k] = true
	}
	for k := range transcripts {
		allKeys[k] = true
	}

	var entries []LinkedEntry
	for key := range allKeys {
		entry := LinkedEntry{
			Recording:  recordings[key],
			Transcript: transcripts[key],
		}
		entries = append(entries, entry)
	}

	// Sort by timestamp, newest first
	sort.Slice(entries, func(i, j int) bool {
		var ti, tj time.Time
		if entries[i].Recording != nil {
			ti = entries[i].Recording.Timestamp
		} else if entries[i].Transcript != nil {
			ti = entries[i].Transcript.Timestamp
		}
		if entries[j].Recording != nil {
			tj = entries[j].Recording.Timestamp
		} else if entries[j].Transcript != nil {
			tj = entries[j].Transcript.Timestamp
		}
		return ti.After(tj)
	})

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(entries)
}
