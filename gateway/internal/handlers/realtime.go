package handlers

import (
	"bytes"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/gorilla/websocket"
	"github.com/prabhanshu/voice-gateway/internal/assemblyai"
	"github.com/prabhanshu/voice-gateway/internal/deepgram"
)

var upgrader = websocket.Upgrader{
	CheckOrigin: func(r *http.Request) bool { return true },
}

// OpenAI Realtime protocol message types (from hyprwhspr).
type realtimeEvent struct {
	Type    string          `json:"type"`
	Session json.RawMessage `json:"session,omitempty"`
	Audio   string          `json:"audio,omitempty"`
}

// RealtimeHandler handles WebSocket connections using the OpenAI Realtime protocol
// and translates them to Deepgram streaming API calls.
func (h *Handler) RealtimeHandler(w http.ResponseWriter, r *http.Request) {
	log.Printf("[Realtime] New WebSocket connection from %s", r.RemoteAddr)

	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Printf("[Realtime] Upgrade failed: %v", err)
		return
	}
	defer conn.Close()

	// Send session.created
	if err := sendJSON(conn, map[string]any{
		"type": "session.created",
		"session": map[string]any{
			"id":     fmt.Sprintf("sess_%d", time.Now().UnixNano()),
			"model":  "nova-2",
			"object": "realtime.session",
		},
	}); err != nil {
		log.Printf("[Realtime] Failed to send session.created: %v", err)
		return
	}

	session := &realtimeSession{
		clientConn:      conn,
		deepgramAPIKey:  h.DeepgramAPIKey,
		localWhisperURL: h.LocalWhisperURL,
		spellings:       h.CustomSpelling,
	}
	defer session.cleanup()

	// Read messages from hyprwhspr
	for {
		_, message, err := conn.ReadMessage()
		if err != nil {
			if websocket.IsCloseError(err, websocket.CloseNormalClosure, websocket.CloseGoingAway) {
				log.Printf("[Realtime] Client disconnected normally")
			} else {
				log.Printf("[Realtime] Read error: %v", err)
			}
			return
		}

		var event realtimeEvent
		if err := json.Unmarshal(message, &event); err != nil {
			log.Printf("[Realtime] Invalid JSON: %v", err)
			continue
		}

		switch event.Type {
		case "session.update":
			session.handleSessionUpdate()
		case "input_audio_buffer.append":
			session.handleAudioAppend(event)
		case "input_audio_buffer.commit":
			session.handleAudioCommit()
		case "input_audio_buffer.clear":
			session.handleAudioClear()
		default:
			log.Printf("[Realtime] Unknown event type: %s", event.Type)
		}
	}
}

// realtimeSession manages state for a single WebSocket session.
type realtimeSession struct {
	clientConn      *websocket.Conn
	clientMu        sync.Mutex
	deepgramAPIKey  string
	localWhisperURL string
	deepgramClient  *deepgram.StreamingClient
	spellings       []assemblyai.CustomSpelling

	// Audio accumulation for archiving and offline transcription
	audioBuffer []byte
	audioMu     sync.Mutex

	// Transcript accumulation from Deepgram finals
	finals   []string
	finalsMu sync.Mutex

	// Deepgram read loop done signal
	readDone chan struct{}

	// Whether session has been configured (session.update received at least once)
	sessionReady bool

	// Offline mode: Deepgram unavailable, use local whisper
	offlineMode     bool
	offlineModeMu   sync.Mutex
	lastDeepgramTry time.Time
}

const deepgramRetryInterval = 30 * time.Second

// connectDeepgram opens a new Deepgram streaming connection and starts the read loop.
// Returns error if connection fails. Sets offlineMode on failure.
func (s *realtimeSession) connectDeepgram() error {
	// Close existing connection if any
	s.closeDeepgram()

	client, err := deepgram.Connect(s.deepgramAPIKey, 24000)
	if err != nil {
		s.offlineModeMu.Lock()
		s.offlineMode = true
		s.lastDeepgramTry = time.Now()
		s.offlineModeMu.Unlock()
		log.Printf("[Realtime] Deepgram unavailable, switching to offline mode: %v", err)
		return err
	}

	// Connection succeeded — clear offline mode
	s.offlineModeMu.Lock()
	wasOffline := s.offlineMode
	s.offlineMode = false
	s.offlineModeMu.Unlock()

	if wasOffline {
		log.Printf("[Realtime] Deepgram connectivity restored, back online")
	}

	s.deepgramClient = client
	s.readDone = make(chan struct{})

	// Reset finals for new utterance
	s.finalsMu.Lock()
	s.finals = nil
	s.finalsMu.Unlock()

	// Start reading Deepgram responses
	go func() {
		defer close(s.readDone)
		err := s.deepgramClient.ReadLoop(
			func(text string) {
				log.Printf("[Deepgram] Interim: %s", text)
			},
			func(text string) {
				log.Printf("[Deepgram] Final: %s", text)
				s.finalsMu.Lock()
				s.finals = append(s.finals, text)
				s.finalsMu.Unlock()
			},
		)
		if err != nil {
			log.Printf("[Deepgram] ReadLoop ended: %v", err)
		}
	}()

	return nil
}

// isOffline returns true if we're in offline mode.
func (s *realtimeSession) isOffline() bool {
	s.offlineModeMu.Lock()
	defer s.offlineModeMu.Unlock()
	return s.offlineMode
}

// shouldRetryDeepgram returns true if enough time has passed to retry Deepgram.
func (s *realtimeSession) shouldRetryDeepgram() bool {
	s.offlineModeMu.Lock()
	defer s.offlineModeMu.Unlock()
	if !s.offlineMode {
		return false
	}
	return time.Since(s.lastDeepgramTry) >= deepgramRetryInterval
}

// closeDeepgram cleanly shuts down the current Deepgram connection.
func (s *realtimeSession) closeDeepgram() {
	if s.deepgramClient != nil {
		s.deepgramClient.Close()
		if s.readDone != nil {
			<-s.readDone
		}
		s.deepgramClient = nil
		s.readDone = nil
	}
}

func (s *realtimeSession) handleSessionUpdate() {
	offline := s.isOffline()
	dgNil := s.deepgramClient == nil
	log.Printf("[Realtime] session.update received (offline=%v, deepgramClient==nil=%v)", offline, dgNil)
	s.sessionReady = true

	// Try to connect to Deepgram for the initial session
	if dgNil && !offline {
		if err := s.connectDeepgram(); err != nil {
			log.Printf("[Realtime] Failed to connect to Deepgram: %v", err)
			// Don't send error to client — we'll fall back to local whisper
			if s.localWhisperURL != "" {
				log.Printf("[Realtime] Will use local whisper fallback at %s", s.localWhisperURL)
			} else {
				s.sendError("deepgram_connection_failed", err.Error())
				return
			}
		}
	}

	// Determine backend for session.updated response
	backend := "nova-2"
	if s.isOffline() {
		backend = "local-whisper"
	}

	// Send session.updated
	s.sendToClient(map[string]any{
		"type": "session.updated",
		"session": map[string]any{
			"model":              backend,
			"input_audio_format": "pcm16",
		},
	})
}

func (s *realtimeSession) handleAudioAppend(event realtimeEvent) {
	if event.Audio == "" {
		return
	}

	if !s.sessionReady {
		log.Printf("[Realtime] Audio received before session.update - ignoring")
		return
	}

	// Decode base64 audio
	pcm, err := base64.StdEncoding.DecodeString(event.Audio)
	if err != nil {
		log.Printf("[Realtime] Base64 decode failed: %v", err)
		return
	}

	// Always accumulate audio (needed for both archiving and offline transcription)
	s.audioMu.Lock()
	s.audioBuffer = append(s.audioBuffer, pcm...)
	bufLen := len(s.audioBuffer)
	s.audioMu.Unlock()

	// If offline, just accumulate — transcription happens at commit time
	if s.isOffline() {
		if bufLen%(48000*2) < len(pcm) { // log roughly every 2s of audio (48000 samples * 2 bytes)
			log.Printf("[Realtime] Offline: accumulating audio, buffer=%d bytes (%.1fs)", bufLen, float64(bufLen)/48000.0)
		}
		return
	}

	// Lazily reconnect to Deepgram if needed (after a previous commit closed it)
	if s.deepgramClient == nil {
		// Try to reconnect (or retry if enough time has passed)
		log.Printf("[Realtime] Reconnecting to Deepgram for new utterance")
		if err := s.connectDeepgram(); err != nil {
			log.Printf("[Realtime] Failed to reconnect to Deepgram: %v (will use offline fallback)", err)
			return
		}
	}

	// Forward to Deepgram
	if err := s.deepgramClient.SendAudio(pcm); err != nil {
		log.Printf("[Realtime] Failed to send audio to Deepgram: %v", err)
	}
}

func (s *realtimeSession) handleAudioCommit() {
	offline := s.isOffline()
	dgNil := s.deepgramClient == nil
	log.Printf("[Realtime] input_audio_buffer.commit received (offline=%v, deepgramClient==nil: %v)", offline, dgNil)

	// Get audio data for potential local transcription
	s.audioMu.Lock()
	audioData := make([]byte, len(s.audioBuffer))
	copy(audioData, s.audioBuffer)
	s.audioMu.Unlock()

	log.Printf("[Realtime] Audio buffer: %d bytes (%.1fs at 24kHz)", len(audioData), float64(len(audioData))/48000.0)

	var fullTranscript string
	var backend string

	if offline || dgNil {
		// OFFLINE PATH: close any stale Deepgram connection before local transcription
		log.Printf("[Realtime] Taking OFFLINE path (offline=%v, deepgramClient==nil=%v, whisperURL=%q)", offline, dgNil, s.localWhisperURL)
		s.closeDeepgram()
		fullTranscript, backend = s.transcribeLocal(audioData)
	} else {
		// ONLINE PATH: finalize Deepgram and collect transcript
		log.Printf("[Realtime] Taking ONLINE path (Deepgram)")
		fullTranscript, backend = s.transcribeDeepgram()
	}

	// Apply custom spelling replacements
	fullTranscript = applySpellingReplacements(fullTranscript, s.spellings)

	log.Printf("[Realtime] Full transcript (%s): %s", backend, fullTranscript)

	// Send transcription result to hyprwhspr
	s.sendToClient(map[string]any{
		"type":          "conversation.item.input_audio_transcription.completed",
		"item_id":       fmt.Sprintf("item_%d", time.Now().UnixNano()),
		"content_index": 0,
		"transcript":    fullTranscript,
	})

	// Clear audio buffer
	s.audioMu.Lock()
	s.audioBuffer = nil
	s.audioMu.Unlock()

	// Archive audio and transcript in background
	go archiveRecording(audioData, fullTranscript, backend)

	// If offline, periodically check if Deepgram is back
	if s.isOffline() && s.shouldRetryDeepgram() {
		go func() {
			log.Printf("[Realtime] Retrying Deepgram connection...")
			if err := s.connectDeepgram(); err != nil {
				log.Printf("[Realtime] Deepgram still unavailable: %v", err)
			}
		}()
	}
}

// transcribeDeepgram finalizes the Deepgram stream and returns the transcript.
func (s *realtimeSession) transcribeDeepgram() (string, string) {
	// Send Finalize to flush remaining audio through Deepgram's pipeline
	if err := s.deepgramClient.Finalize(); err != nil {
		log.Printf("[Realtime] Finalize failed: %v", err)
	}

	// Wait for Deepgram to send back remaining finals after Finalize
	time.Sleep(1500 * time.Millisecond)

	// Close Deepgram connection to ensure all responses are received
	s.deepgramClient.Close()
	if s.readDone != nil {
		<-s.readDone
	}
	s.deepgramClient = nil
	s.readDone = nil

	// Collect full transcript
	s.finalsMu.Lock()
	fullTranscript := strings.Join(s.finals, " ")
	s.finals = nil
	s.finalsMu.Unlock()

	return fullTranscript, "deepgram"
}

// transcribeLocal sends accumulated audio to the local whisper server.
func (s *realtimeSession) transcribeLocal(audioData []byte) (string, string) {
	if s.localWhisperURL == "" {
		log.Printf("[Whisper] FAIL: No local whisper URL configured (localWhisperURL is empty)")
		return "", "none"
	}

	if len(audioData) == 0 {
		log.Printf("[Whisper] FAIL: audioData is empty (0 bytes) — nothing to transcribe")
		return "", "local-whisper"
	}

	// Build WAV in memory
	wavBuf := buildWAV(audioData, 24000)
	log.Printf("[Whisper] Built WAV: %d bytes (PCM: %d bytes, %.1fs audio at 24kHz)", len(wavBuf), len(audioData), float64(len(audioData))/48000.0)

	// POST to local whisper server
	url := s.localWhisperURL + "/transcribe"
	log.Printf("[Whisper] POSTing to %s ...", url)
	t0 := time.Now()

	client := &http.Client{Timeout: 120 * time.Second}
	resp, err := client.Post(url, "audio/wav", bytes.NewReader(wavBuf))
	elapsed := time.Since(t0)
	if err != nil {
		log.Printf("[Whisper] FAIL: POST to %s failed after %v: %v", url, elapsed, err)
		return "", "local-whisper"
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		log.Printf("[Whisper] FAIL: reading response body failed after %v: %v", elapsed, err)
		return "", "local-whisper"
	}

	log.Printf("[Whisper] Response: status=%d, body=%s, roundtrip=%v", resp.StatusCode, string(body), elapsed)

	if resp.StatusCode != http.StatusOK {
		log.Printf("[Whisper] FAIL: non-200 status %d from whisper server", resp.StatusCode)
		return "", "local-whisper"
	}

	var result struct {
		Text           string  `json:"text"`
		Model          string  `json:"model"`
		Duration       float64 `json:"duration"`
		TranscribeTime float64 `json:"transcribe_time"`
	}
	if err := json.Unmarshal(body, &result); err != nil {
		log.Printf("[Whisper] FAIL: JSON parse error: %v (body was: %s)", err, string(body))
		return "", "local-whisper"
	}

	log.Printf("[Whisper] OK: text=%q, model=%s, audio=%.1fs, transcribe=%.2fs, roundtrip=%v", result.Text, result.Model, result.Duration, result.TranscribeTime, elapsed)
	return result.Text, "local-whisper"
}

// buildWAV creates a WAV file in memory from PCM16 audio data.
func buildWAV(pcmData []byte, sampleRate int) []byte {
	var buf bytes.Buffer

	channels := uint16(1)
	bitsPerSample := uint16(16)
	byteRate := uint32(sampleRate) * uint32(channels) * uint32(bitsPerSample/8)
	blockAlign := channels * (bitsPerSample / 8)
	dataSize := uint32(len(pcmData))

	// RIFF header
	buf.Write([]byte("RIFF"))
	binary.Write(&buf, binary.LittleEndian, uint32(36+dataSize))
	buf.Write([]byte("WAVE"))

	// fmt chunk
	buf.Write([]byte("fmt "))
	binary.Write(&buf, binary.LittleEndian, uint32(16))
	binary.Write(&buf, binary.LittleEndian, uint16(1))
	binary.Write(&buf, binary.LittleEndian, channels)
	binary.Write(&buf, binary.LittleEndian, uint32(sampleRate))
	binary.Write(&buf, binary.LittleEndian, byteRate)
	binary.Write(&buf, binary.LittleEndian, blockAlign)
	binary.Write(&buf, binary.LittleEndian, bitsPerSample)

	// data chunk
	buf.Write([]byte("data"))
	binary.Write(&buf, binary.LittleEndian, dataSize)
	buf.Write(pcmData)

	return buf.Bytes()
}

func (s *realtimeSession) handleAudioClear() {
	log.Printf("[Realtime] input_audio_buffer.clear received")

	// Reset audio archive buffer
	s.audioMu.Lock()
	s.audioBuffer = nil
	s.audioMu.Unlock()

	// Close existing Deepgram connection (will reconnect lazily on first append)
	s.closeDeepgram()

	s.finalsMu.Lock()
	s.finals = nil
	s.finalsMu.Unlock()
}

func (s *realtimeSession) cleanup() {
	s.closeDeepgram()
}

func (s *realtimeSession) sendToClient(msg any) {
	s.clientMu.Lock()
	defer s.clientMu.Unlock()
	if err := s.clientConn.WriteJSON(msg); err != nil {
		log.Printf("[Realtime] Failed to send to client: %v", err)
	}
}

func (s *realtimeSession) sendError(code, message string) {
	s.sendToClient(map[string]any{
		"type": "error",
		"error": map[string]any{
			"type":    code,
			"message": message,
		},
	})
}

// archiveRecording saves audio as WAV and transcript as text file.
func archiveRecording(audioData []byte, transcript string, backend string) {
	timestamp := time.Now().Format("20060102_150405")

	if len(audioData) > 0 {
		if err := os.MkdirAll("../recordings", 0755); err != nil {
			log.Printf("[Archive] Failed to create recordings dir: %v", err)
		} else {
			wavPath := filepath.Join("../recordings", fmt.Sprintf("%s_audio.wav", timestamp))
			if err := writeWAV(wavPath, audioData, 24000); err != nil {
				log.Printf("[Archive] Failed to save WAV: %v", err)
			} else {
				log.Printf("[Archive] Saved audio: %s", wavPath)
			}
		}
	}

	if transcript != "" {
		if err := os.MkdirAll("../transcripts", 0755); err != nil {
			log.Printf("[Archive] Failed to create transcripts dir: %v", err)
		} else {
			txtPath := filepath.Join("../transcripts", fmt.Sprintf("%s_%s.txt", timestamp, backend))
			if err := os.WriteFile(txtPath, []byte(transcript), 0644); err != nil {
				log.Printf("[Archive] Failed to save transcript: %v", err)
			} else {
				log.Printf("[Archive] Saved transcript: %s", txtPath)
			}
		}
	}
}

// writeWAV writes PCM16 audio data as a WAV file.
func writeWAV(path string, pcmData []byte, sampleRate int) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()

	channels := uint16(1)
	bitsPerSample := uint16(16)
	byteRate := uint32(sampleRate) * uint32(channels) * uint32(bitsPerSample/8)
	blockAlign := channels * (bitsPerSample / 8)
	dataSize := uint32(len(pcmData))

	// RIFF header
	f.Write([]byte("RIFF"))
	binary.Write(f, binary.LittleEndian, uint32(36+dataSize))
	f.Write([]byte("WAVE"))

	// fmt chunk
	f.Write([]byte("fmt "))
	binary.Write(f, binary.LittleEndian, uint32(16)) // chunk size
	binary.Write(f, binary.LittleEndian, uint16(1))  // PCM format
	binary.Write(f, binary.LittleEndian, channels)
	binary.Write(f, binary.LittleEndian, uint32(sampleRate))
	binary.Write(f, binary.LittleEndian, byteRate)
	binary.Write(f, binary.LittleEndian, blockAlign)
	binary.Write(f, binary.LittleEndian, bitsPerSample)

	// data chunk
	f.Write([]byte("data"))
	binary.Write(f, binary.LittleEndian, dataSize)
	f.Write(pcmData)

	return nil
}

func sendJSON(conn *websocket.Conn, v any) error {
	return conn.WriteJSON(v)
}

// applySpellingReplacements applies custom spelling corrections to text.
func applySpellingReplacements(text string, spellings []assemblyai.CustomSpelling) string {
	for _, s := range spellings {
		text = strings.ReplaceAll(text, s.From, s.To)
	}
	return text
}
