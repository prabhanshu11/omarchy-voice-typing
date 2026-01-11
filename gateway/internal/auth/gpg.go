package auth

import (
	"bytes"
	"fmt"
	"log"
	"os"
	"os/exec"
	"strings"
	"sync"
)

var (
	cachedAPIKey string
	keyMutex     sync.Mutex
	unlockOnce   sync.Once
)

// LoadAPIKeyWithPrompt attempts to load the API key from pass.
// If GPG is locked, it spawns a terminal window to prompt for unlock.
func LoadAPIKeyWithPrompt() (string, error) {
	keyMutex.Lock()
	defer keyMutex.Unlock()

	// Return cached key if already loaded
	if cachedAPIKey != "" {
		return cachedAPIKey, nil
	}

	// Check environment variables first
	apiKey := os.Getenv("ASSEMBLYAI_API_KEY")
	if apiKey == "" {
		apiKey = os.Getenv("ASSEMBLY_API_KEY")
	}
	if apiKey != "" {
		cachedAPIKey = apiKey
		return cachedAPIKey, nil
	}

	// Try to load from pass silently (non-blocking)
	key, err := tryLoadFromPass()
	if err == nil && key != "" {
		cachedAPIKey = key
		log.Printf("API key loaded from pass (GPG already unlocked)")
		return cachedAPIKey, nil
	}

	// GPG is locked - prompt user to unlock
	log.Printf("GPG appears locked - prompting user to unlock...")
	if err := promptGPGUnlock(); err != nil {
		return "", fmt.Errorf("failed to prompt for GPG unlock: %w", err)
	}

	// Try loading again after unlock
	key, err = tryLoadFromPass()
	if err != nil {
		return "", fmt.Errorf("failed to load API key from pass after unlock: %w", err)
	}
	if key == "" {
		return "", fmt.Errorf("API key is empty in pass")
	}

	cachedAPIKey = key
	log.Printf("API key successfully loaded from pass")
	return cachedAPIKey, nil
}

// tryLoadFromPass attempts to load the API key from pass with a short timeout.
// Returns empty string if it times out (GPG locked).
func tryLoadFromPass() (string, error) {
	// Try with timeout to avoid blocking
	cmd := exec.Command("timeout", "2", "pass", "api/assemblyai")
	var out bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &out
	cmd.Stderr = &stderr

	err := cmd.Run()
	if err != nil {
		// Timeout or other error
		if strings.Contains(stderr.String(), "gpg") || strings.Contains(stderr.String(), "timed out") {
			return "", fmt.Errorf("GPG locked or timeout")
		}
		return "", fmt.Errorf("pass command failed: %w (stderr: %s)", err, stderr.String())
	}

	key := strings.TrimSpace(out.String())
	return key, nil
}

// promptGPGUnlock opens a terminal window asking the user to unlock GPG.
func promptGPGUnlock() error {
	// Detect terminal emulator (prefer ghostty on Omarchy)
	terminal := detectTerminal()

	// Create a script that prompts for GPG unlock
	script := `#!/bin/bash
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🔐 Voice Gateway: GPG Unlock Required"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Voice typing needs your AssemblyAI API key."
echo "Please unlock your GPG keyring to continue."
echo ""
echo "Running: pass api/assemblyai"
echo ""
pass api/assemblyai > /dev/null
if [ $? -eq 0 ]; then
    echo ""
    echo "✅ GPG unlocked successfully!"
    echo ""
    echo "You can now close this window."
    echo "Press Enter to continue..."
    read
else
    echo ""
    echo "❌ Failed to unlock GPG"
    echo ""
    echo "Press Enter to exit..."
    read
    exit 1
fi
`

	// Write script to temporary file
	tmpScript := "/tmp/voice-gateway-gpg-unlock.sh"
	if err := os.WriteFile(tmpScript, []byte(script), 0755); err != nil {
		return fmt.Errorf("failed to create unlock script: %w", err)
	}
	defer os.Remove(tmpScript)

	// Launch terminal with the script
	log.Printf("Spawning terminal: %s", terminal)
	var cmd *exec.Cmd

	switch terminal {
	case "ghostty":
		cmd = exec.Command("ghostty", "--command", "bash", tmpScript)
	case "kitty":
		cmd = exec.Command("kitty", "--hold", "bash", tmpScript)
	case "alacritty":
		cmd = exec.Command("alacritty", "-e", "bash", tmpScript)
	default:
		// Fallback to xterm or gnome-terminal
		cmd = exec.Command("x-terminal-emulator", "-e", "bash", tmpScript)
	}

	// Run the terminal and wait for it to close
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("terminal command failed: %w", err)
	}

	return nil
}

// detectTerminal finds the available terminal emulator.
func detectTerminal() string {
	terminals := []string{"ghostty", "kitty", "alacritty", "gnome-terminal", "xterm"}
	for _, term := range terminals {
		if _, err := exec.LookPath(term); err == nil {
			return term
		}
	}
	return "xterm" // fallback
}
