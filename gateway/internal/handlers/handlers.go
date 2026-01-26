package handlers

import (
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/prabhanshu/voice-gateway/internal/assemblyai"
	"github.com/prabhanshu/voice-gateway/internal/auth"
)

type TranscribeRequest struct {
	AudioURL string `json:"audio_url"`
}

type TranscribeResponse struct {
	Text      string                     `json:"text"`
	Raw       *assemblyai.TranscriptResponse `json:"raw"`
	DurationS float64                    `json:"duration_s"`
}

type ErrorResponse struct {
	Error   string `json:"error"`
	Details string `json:"details,omitempty"`
}

type Handler struct {
	AAIClient      *assemblyai.Client
	CustomSpelling []assemblyai.CustomSpelling
	clientMutex    sync.Mutex
}

type ReplacementConfig struct {
	From []string `json:"from"`
	To   string   `json:"to"`
}

func LoadReplacements(path string) ([]assemblyai.CustomSpelling, error) {
	file, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}
	defer file.Close()

	var configs []ReplacementConfig
	if err := json.NewDecoder(file).Decode(&configs); err != nil {
		return nil, err
	}

	var spellings []assemblyai.CustomSpelling
	for _, c := range configs {
		for _, f := range c.From {
			spellings = append(spellings, assemblyai.CustomSpelling{
				From: f,
				To:   c.To,
			})
		}
	}
	return spellings, nil
}

// ensureClient ensures the AAIClient is initialized.
// If not initialized, it attempts to load the API key from pass with GPG unlock prompt.
func (h *Handler) ensureClient() error {
	h.clientMutex.Lock()
	defer h.clientMutex.Unlock()

	// Already initialized
	if h.AAIClient != nil {
		return nil
	}

	log.Printf("[GPG] API client not initialized - attempting to load API key from pass")

	// Load API key with GPG unlock prompt
	apiKey, err := auth.LoadAPIKeyWithPrompt()
	if err != nil {
		return fmt.Errorf("failed to load API key: %w", err)
	}

	// Initialize client
	h.AAIClient = assemblyai.NewClient(apiKey)
	log.Printf("[GPG] API client initialized successfully")

	return nil
}

func (h *Handler) TranscribeHandler(w http.ResponseWriter, r *http.Request) {
	log.Printf("Received %s request to %s", r.Method, r.URL.Path)
	if r.Method != http.MethodPost {
		http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
		return
	}

	// Ensure API client is initialized (loads from pass if needed)
	if err := h.ensureClient(); err != nil {
		log.Printf("[ERROR] Failed to initialize API client: %v", err)
		h.sendError(w, http.StatusInternalServerError, "Failed to load API key", err.Error())
		return
	}

	var audioURL string

	// Check if it's multipart/form-data
	if contentType := r.Header.Get("Content-Type"); contentType != "" && (contentType[:19] == "multipart/form-data" || contentType[:33] == "application/x-www-form-urlencoded") {
		err := r.ParseMultipartForm(32 << 20) // 32MB max in memory
		if err != nil {
			h.sendError(w, http.StatusBadRequest, "Failed to parse multipart form", err.Error())
			return
		}

		file, header, err := r.FormFile("file")
		if err == nil {
			defer file.Close()

			// Save audio locally
			if err := os.MkdirAll("../recordings", 0755); err != nil {
				log.Printf("Failed to create recordings dir: %v", err)
			}
			localFilename := fmt.Sprintf("%s_%s", time.Now().Format("20060102_150405"), header.Filename)
			localPath := filepath.Join("../recordings", localFilename)
			
			outFile, err := os.Create(localPath)
			if err != nil {
				log.Printf("Failed to create local audio file: %v", err)
				// Continue with upload even if save fails, but maybe better to fail? 
				// Proceeding is safer for UX.
			} else {
				defer outFile.Close()
			}

			var reader io.Reader = file
			if outFile != nil {
				reader = io.TeeReader(file, outFile)
			}

			// Use context from request for the client call
			url, err := h.AAIClient.UploadWithContext(r.Context(), reader)
			if err != nil {
				h.sendError(w, http.StatusInternalServerError, "Failed to upload to AssemblyAI", err.Error())
				return
			}
			audioURL = url
		}
	}

	// If audioURL is still empty, check for JSON body
	if audioURL == "" {
		var req TranscribeRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err == nil && req.AudioURL != "" {
			audioURL = req.AudioURL
		}
	}

	if audioURL == "" {
		h.sendError(w, http.StatusBadRequest, "No audio file or URL provided", "")
		return
	}

	log.Printf("Processing transcription for audio URL: %s", audioURL)

	// Create transcript
	transcriptID, err := h.AAIClient.CreateTranscriptWithContext(r.Context(), audioURL, h.CustomSpelling)
	if err != nil {
		h.sendError(w, http.StatusInternalServerError, "Failed to create transcript", err.Error())
		return
	}
	log.Printf("Transcript job created with ID: %s", transcriptID)

	// Poll for completion
	finalTranscript, err := h.pollTranscript(transcriptID)
	if err != nil {
		h.sendError(w, http.StatusInternalServerError, "Transcription failed or timed out", err.Error())
		return
	}
	log.Printf("Transcription completed for ID: %s. Duration: %.2fs", transcriptID, finalTranscript.AudioDuration)

	// Save transcript locally
	if err := os.MkdirAll("../transcripts", 0755); err != nil {
		log.Printf("Failed to create transcripts dir: %v", err)
	}
	txtFilename := fmt.Sprintf("%s_%s.txt", time.Now().Format("20060102_150405"), transcriptID)
	txtPath := filepath.Join("../transcripts", txtFilename)
	if err := os.WriteFile(txtPath, []byte(finalTranscript.Text), 0644); err != nil {
		log.Printf("Failed to save transcript: %v", err)
	}

	resp := TranscribeResponse{
		Text:      finalTranscript.Text,
		Raw:       finalTranscript,
		DurationS: finalTranscript.AudioDuration,
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(resp)
}

func (h *Handler) pollTranscript(id string) (*assemblyai.TranscriptResponse, error) {
	// Start with fast polling (350ms), then back off to reduce API load
	// This reduces latency from ~2s to ~350ms for quick transcriptions
	initialInterval := 350 * time.Millisecond
	maxInterval := 2 * time.Second
	currentInterval := initialInterval

	ticker := time.NewTicker(currentInterval)
	defer ticker.Stop()

	timeout := time.After(300 * time.Second)

	for {
		select {
		case <-timeout:
			return nil, fmt.Errorf("transcription timed out after 300 seconds")
		case <-ticker.C:
			res, err := h.AAIClient.GetTranscript(id)
			if err != nil {
				return nil, err
			}

			if res.Status == "completed" {
				return res, nil
			}

			if res.Status == "error" {
				return nil, fmt.Errorf("assemblyai error: %s", res.Error)
			}

			// Exponential backoff: increase interval up to maxInterval
			if currentInterval < maxInterval {
				currentInterval = currentInterval * 3 / 2 // 1.5x increase
				if currentInterval > maxInterval {
					currentInterval = maxInterval
				}
				ticker.Reset(currentInterval)
			}
		}
	}
}

func (h *Handler) sendError(w http.ResponseWriter, code int, message string, details string) {
	log.Printf("Error: %s, Details: %s", message, details)
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	json.NewEncoder(w).Encode(ErrorResponse{
		Error:   message,
		Details: details,
	})
}
