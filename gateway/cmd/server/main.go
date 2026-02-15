package main

import (
	"bytes"
	"encoding/json"
	"log"
	"net/http"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/prabhanshu/voice-gateway/internal/assemblyai"
	"github.com/prabhanshu/voice-gateway/internal/handlers"
)

func main() {
	// Load AssemblyAI API key (for REST endpoint)
	apiKey := os.Getenv("ASSEMBLYAI_API_KEY")
	if apiKey == "" {
		apiKey = os.Getenv("ASSEMBLY_API_KEY")
	}

	if apiKey == "" {
		log.Printf("No AssemblyAI API key in environment - will load from pass on first REST request")
	}

	var aaiClient *assemblyai.Client
	if apiKey != "" {
		aaiClient = assemblyai.NewClient(apiKey)
		log.Printf("AssemblyAI API key loaded from environment")
	}

	// Load Deepgram API key (for streaming WebSocket endpoint)
	deepgramKey := os.Getenv("DEEPGRAM_API_KEY")
	if deepgramKey == "" {
		deepgramKey = loadFromPass("api/deepgram")
	}
	if deepgramKey != "" {
		log.Printf("Deepgram API key loaded")
	} else {
		log.Printf("Warning: No Deepgram API key found - streaming endpoint will not work")
	}

	// Local whisper server URL for offline fallback
	localWhisperURL := os.Getenv("LOCAL_WHISPER_URL")
	if localWhisperURL == "" {
		localWhisperURL = "http://localhost:8767"
	}
	log.Printf("Local whisper fallback URL: %s", localWhisperURL)

	// LAN whisper server URL (e.g., desktop GPU via Tailscale)
	lanWhisperURL := os.Getenv("LAN_WHISPER_URL")
	if lanWhisperURL != "" {
		log.Printf("LAN whisper fallback URL: %s", lanWhisperURL)
	} else {
		log.Printf("LAN whisper fallback: not configured (set LAN_WHISPER_URL to enable)")
	}

	replacementsPath := "../config/replacements.json"
	customSpelling, err := handlers.LoadReplacements(replacementsPath)
	if err != nil {
		log.Printf("Warning: Failed to load replacements from %s: %v", replacementsPath, err)
	} else {
		log.Printf("Loaded %d custom spelling replacements", len(customSpelling))
	}

	h := &handlers.Handler{
		AAIClient:       aaiClient,
		CustomSpelling:  customSpelling,
		DeepgramAPIKey:  deepgramKey,
		LocalWhisperURL: localWhisperURL,
		LANWhisperURL:   lanWhisperURL,
	}

	http.HandleFunc("/v1/transcribe", h.TranscribeHandler)
	http.HandleFunc("/v1/realtime", h.RealtimeHandler)
	http.HandleFunc("/health", healthHandler(localWhisperURL, lanWhisperURL))

	port := os.Getenv("PORT")
	if port == "" {
		port = "8765"
	}

	log.Printf("Starting gateway server on :%s", port)
	server := &http.Server{
		Addr:         ":" + port,
		WriteTimeout: 600 * time.Second,
		ReadTimeout:  600 * time.Second,
		IdleTimeout:  600 * time.Second,
	}
	if err := server.ListenAndServe(); err != nil {
		log.Fatalf("Server failed to start: %v", err)
	}
}

// healthHandler returns a handler that reports gateway health and backend availability.
func healthHandler(localWhisperURL, lanWhisperURL string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		backend := "deepgram"
		client := &http.Client{Timeout: 2 * time.Second}

		// Check if local whisper is available
		whisperReady := false
		if localWhisperURL != "" {
			resp, err := client.Get(localWhisperURL + "/health")
			if err == nil {
				resp.Body.Close()
				if resp.StatusCode == http.StatusOK {
					whisperReady = true
				}
			}
		}

		// Check if LAN whisper is available
		lanWhisperReady := false
		if lanWhisperURL != "" {
			resp, err := client.Get(lanWhisperURL + "/health")
			if err == nil {
				resp.Body.Close()
				if resp.StatusCode == http.StatusOK {
					lanWhisperReady = true
				}
			}
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]any{
			"status":            "ok",
			"backend":           backend,
			"whisper_ready":     whisperReady,
			"whisper_url":       localWhisperURL,
			"lan_whisper_ready": lanWhisperReady,
			"lan_whisper_url":   lanWhisperURL,
		})
	}
}

// loadFromPass attempts to load a secret from pass with a short timeout.
func loadFromPass(path string) string {
	cmd := exec.Command("timeout", "2", "pass", path)
	var out bytes.Buffer
	cmd.Stdout = &out
	cmd.Stderr = nil
	if err := cmd.Run(); err != nil {
		return ""
	}
	return strings.TrimSpace(out.String())
}
