package deepgram

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"net/url"
	"strings"
	"sync"

	"github.com/gorilla/websocket"
)

const deepgramWSURL = "wss://api.deepgram.com/v1/listen"

// StreamingClient manages a WebSocket connection to Deepgram's streaming API.
type StreamingClient struct {
	conn   *websocket.Conn
	mu     sync.Mutex
	closed bool
}

// Response represents a Deepgram streaming response.
type Response struct {
	Type    string  `json:"type"`
	Channel Channel `json:"channel"`
	IsFinal bool    `json:"is_final"`
}

// Channel contains alternatives from Deepgram.
type Channel struct {
	Alternatives []Alternative `json:"alternatives"`
}

// Alternative contains a transcript result.
type Alternative struct {
	Transcript string  `json:"transcript"`
	Confidence float64 `json:"confidence"`
}

// Connect opens a WebSocket connection to Deepgram's streaming API.
func Connect(apiKey string, sampleRate int) (*StreamingClient, error) {
	if sampleRate == 0 {
		sampleRate = 24000
	}

	params := url.Values{}
	params.Set("model", "nova-2")
	params.Set("encoding", "linear16")
	params.Set("sample_rate", fmt.Sprintf("%d", sampleRate))
	params.Set("channels", "1")
	params.Set("interim_results", "true")
	params.Set("punctuate", "true")
	params.Set("smart_format", "true")
	params.Set("endpointing", "300")

	wsURL := deepgramWSURL + "?" + params.Encode()

	header := http.Header{}
	header.Set("Authorization", "Token "+apiKey)

	conn, resp, err := websocket.DefaultDialer.Dial(wsURL, header)
	if err != nil {
		if resp != nil {
			return nil, fmt.Errorf("deepgram connect failed (status %d): %w", resp.StatusCode, err)
		}
		return nil, fmt.Errorf("deepgram connect failed: %w", err)
	}

	log.Printf("[Deepgram] Connected to streaming API (sample_rate=%d)", sampleRate)
	return &StreamingClient{conn: conn}, nil
}

// SendAudio sends raw PCM16 audio bytes to Deepgram.
func (c *StreamingClient) SendAudio(pcm16 []byte) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed {
		return fmt.Errorf("connection closed")
	}
	return c.conn.WriteMessage(websocket.BinaryMessage, pcm16)
}

// Finalize sends a Finalize message to flush remaining audio through the pipeline.
func (c *StreamingClient) Finalize() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed {
		return fmt.Errorf("connection closed")
	}
	msg := map[string]string{"type": "Finalize"}
	return c.conn.WriteJSON(msg)
}

// Close sends CloseStream and closes the WebSocket connection.
func (c *StreamingClient) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed {
		return nil
	}
	c.closed = true
	// Send CloseStream message (best effort)
	msg := map[string]string{"type": "CloseStream"}
	_ = c.conn.WriteJSON(msg)
	return c.conn.Close()
}

// ReadLoop reads Deepgram responses and calls onInterim/onFinal callbacks.
// It blocks until the connection is closed or an error occurs.
func (c *StreamingClient) ReadLoop(onInterim, onFinal func(text string)) error {
	for {
		_, message, err := c.conn.ReadMessage()
		if err != nil {
			if c.closed {
				return nil
			}
			if websocket.IsCloseError(err, websocket.CloseNormalClosure, websocket.CloseGoingAway) {
				return nil
			}
			return fmt.Errorf("deepgram read error: %w", err)
		}

		var resp Response
		if err := json.Unmarshal(message, &resp); err != nil {
			log.Printf("[Deepgram] Failed to parse response: %v", err)
			continue
		}

		if resp.Type != "Results" {
			continue
		}

		if len(resp.Channel.Alternatives) == 0 {
			continue
		}

		transcript := strings.TrimSpace(resp.Channel.Alternatives[0].Transcript)
		if transcript == "" {
			continue
		}

		if resp.IsFinal {
			if onFinal != nil {
				onFinal(transcript)
			}
		} else {
			if onInterim != nil {
				onInterim(transcript)
			}
		}
	}
}
