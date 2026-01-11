package main

import (
	"log"
	"net/http"
	"os"
	"time"

	"github.com/prabhanshu/voice-gateway/internal/assemblyai"
	"github.com/prabhanshu/voice-gateway/internal/handlers"
)

func main() {
	// Try loading API key from environment first
	apiKey := os.Getenv("ASSEMBLYAI_API_KEY")
	if apiKey == "" {
		apiKey = os.Getenv("ASSEMBLY_API_KEY")
	}

	// If no API key in environment, try loading from pass (non-blocking check)
	if apiKey == "" {
		log.Printf("No API key in environment - will load from pass on first request")
	}

	var aaiClient *assemblyai.Client
	if apiKey != "" {
		aaiClient = assemblyai.NewClient(apiKey)
		log.Printf("API key loaded from environment")
	}

	replacementsPath := "../config/replacements.json"
	customSpelling, err := handlers.LoadReplacements(replacementsPath)
	if err != nil {
		log.Printf("Warning: Failed to load replacements from %s: %v", replacementsPath, err)
	} else {
		log.Printf("Loaded %d custom spelling replacements", len(customSpelling))
	}

	h := &handlers.Handler{
		AAIClient:      aaiClient,
		CustomSpelling: customSpelling,
	}

	http.HandleFunc("/v1/transcribe", h.TranscribeHandler)

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
