package assemblyai

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"
)

const baseURL = "https://api.assemblyai.com/v2"

type Client struct {
	APIKey string
	HTTP   *http.Client
}

func NewClient(apiKey string) *Client {
	return &Client{
		APIKey: apiKey,
		HTTP: &http.Client{
			Timeout: 300 * time.Second,
		},
	}
}

type UploadResponse struct {
	UploadURL string `json:"upload_url"`
}

type TranscriptRequest struct {
	AudioURL          string           `json:"audio_url"`
	Punctuate         bool             `json:"punctuate"`
	FormatText        bool             `json:"format_text"`
	LanguageDetection bool             `json:"language_detection"`
	CustomSpelling    []CustomSpelling `json:"custom_spelling,omitempty"`
}

type CustomSpelling struct {
	From string `json:"from"`
	To   string `json:"to"`
}

type TranscriptResponse struct {
	ID         string          `json:"id"`
	Status     string          `json:"status"`
	Text       string          `json:"text"`
	Error      string          `json:"error"`
	AudioDuration float64      `json:"audio_duration"`
}

func (c *Client) Upload(audio io.Reader) (string, error) {
	return c.UploadWithContext(context.Background(), audio)
}

func (c *Client) UploadWithContext(ctx context.Context, audio io.Reader) (string, error) {
	req, err := http.NewRequestWithContext(ctx, "POST", baseURL+"/upload", audio)
	if err != nil {
		return "", err
	}

	req.Header.Set("Authorization", c.APIKey)
	req.Header.Set("Content-Type", "application/octet-stream")

	resp, err := c.HTTP.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return "", fmt.Errorf("upload failed with status %d: %s", resp.StatusCode, string(body))
	}

	var ur UploadResponse
	if err := json.NewDecoder(resp.Body).Decode(&ur); err != nil {
		return "", err
	}

	return ur.UploadURL, nil
}

func (c *Client) CreateTranscript(audioURL string) (string, error) {
	return c.CreateTranscriptWithContext(context.Background(), audioURL, nil)
}

func (c *Client) CreateTranscriptWithContext(ctx context.Context, audioURL string, customSpelling []CustomSpelling) (string, error) {
	data := TranscriptRequest{
		AudioURL:          audioURL,
		Punctuate:         true,
		FormatText:        true,
		LanguageDetection: true,
		CustomSpelling:    customSpelling,
	}

	body, err := json.Marshal(data)
	if err != nil {
		return "", err
	}

	req, err := http.NewRequestWithContext(ctx, "POST", baseURL+"/transcript", bytes.NewBuffer(body))
	if err != nil {
		return "", err
	}

	req.Header.Set("Authorization", c.APIKey)
	req.Header.Set("Content-Type", "application/json")

	resp, err := c.HTTP.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return "", fmt.Errorf("transcript creation failed with status %d: %s", resp.StatusCode, string(body))
	}

	var tr TranscriptResponse
	if err := json.NewDecoder(resp.Body).Decode(&tr); err != nil {
		return "", err
	}

	return tr.ID, nil
}


func (c *Client) GetTranscript(id string) (*TranscriptResponse, error) {
	req, err := http.NewRequest("GET", baseURL+"/transcript/"+id, nil)
	if err != nil {
		return nil, err
	}

	req.Header.Set("Authorization", c.APIKey)

	resp, err := c.HTTP.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("getting transcript failed with status %d: %s", resp.StatusCode, string(body))
	}

	var tr TranscriptResponse
	if err := json.NewDecoder(resp.Body).Decode(&tr); err != nil {
		return nil, err
	}

	return &tr, nil
}
