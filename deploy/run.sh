#!/bin/bash
set -e

echo "Starting voice-type deployment..."

cd /var/www/voice-type

echo "Building Docker image..."
docker build -t voice-type .

echo "Stopping existing container..."
docker stop voice-type || true
docker rm voice-type || true

echo "Running voice-type container..."
docker run -d \
  --name voice-type \
  --restart always \
  -p 8766:8766 \
  -e PORT=8766 \
  -e RUST_LOG=info \
  voice-type

echo "Waiting for startup..."
sleep 3

echo "Health check..."
if curl -f http://localhost:8766/health > /dev/null 2>&1; then
    echo "voice-type is healthy!"
else
    echo "Health check failed!"
    docker logs voice-type
    exit 1
fi

echo "voice-type deployment complete!"
