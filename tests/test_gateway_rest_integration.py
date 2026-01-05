import requests
import os
import pytest

GATEWAY_URL = os.getenv("GATEWAY_URL", "http://127.0.0.1:8765")
AUDIO_FILE_WAV = "Harvard_speech100.wav"
AUDIO_FILE_MP3 = "Harvard_speech100.mp3"

@pytest.mark.parametrize("audio_file", [AUDIO_FILE_WAV, AUDIO_FILE_MP3])
def test_transcribe_file(audio_file):
    # Ensure audio file exists
    assert os.path.exists(audio_file), f"{audio_file} not found at project root"

    url = f"{GATEWAY_URL}/v1/transcribe"
    
    with open(audio_file, "rb") as f:
        content_type = "audio/wav" if audio_file.endswith(".wav") else "audio/mpeg"
        files = {"file": (audio_file, f, content_type)}
        response = requests.post(url, files=files, timeout=300)

    assert response.status_code == 200, f"Expected 200, got {response.status_code}: {response.text}"
    
    data = response.json()
    assert "text" in data
    assert len(data["text"]) > 0
    
    # Save output to logs
    os.makedirs("logs", exist_ok=True)
    output_path = f"logs/{os.path.basename(audio_file)}.txt"
    with open(output_path, "w") as f:
        f.write(data["text"])
        
    print(f"\nFile: {audio_file}")
    print(f"Transcription saved to: {output_path}")
    print(f"Duration: {data.get('duration_s')}s")

if __name__ == "__main__":
    # If run directly, just run the test
    test_transcribe_file()
