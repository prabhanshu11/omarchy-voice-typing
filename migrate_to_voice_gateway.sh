#!/bin/bash
set -e

# Configuration
OLD_NAME="omarchy"
NEW_NAME="voice-gateway"
OLD_SERVICE="omarchy-gateway"
NEW_SERVICE="voice-gateway"
PROJECT_DIR="omarchy-voice-typing" # Current folder name

echo ">>> Stopping old service..."
sudo systemctl disable --now $OLD_SERVICE || true
sudo rm -f /etc/systemd/system/$OLD_SERVICE.service

echo ">>> Migrating /etc configuration..."
if [ -d "/etc/$OLD_NAME" ]; then
    sudo mv "/etc/$OLD_NAME" "/etc/$NEW_NAME"
    if [ -f "/etc/$NEW_NAME/$OLD_NAME.env" ]; then
        sudo mv "/etc/$NEW_NAME/$OLD_NAME.env" "/etc/$NEW_NAME/service.env"
    fi
    # Update the env file variable name key if strictly needed, but usually the key inside is ASSEMBLYAI_API_KEY which is fine.
fi

echo ">>> Renaming local files..."
# Rename systemd file
if [ -f "gateway/systemd/$OLD_SERVICE.service" ]; then
    mv "gateway/systemd/$OLD_SERVICE.service" "gateway/systemd/$NEW_SERVICE.service"
fi

# Find and replace text in files
# Exclude .git and the script itself
grep -rIl "$OLD_NAME" . | grep -vE "^./.git" | grep -v "migrate_to_voice_gateway.sh" | xargs sed -i "s/$OLD_NAME/$NEW_NAME/g"
grep -rIl "Omarchy" . | grep -vE "^./.git" | grep -v "migrate_to_voice_gateway.sh" | xargs sed -i "s/Omarchy/Voice Gateway/g"

# Fix binary name reference in systemd service explicitly if missed
sed -i "s/$OLD_SERVICE/$NEW_SERVICE/g" "gateway/systemd/$NEW_SERVICE.service"
# Update EnvironmentFile path in systemd
sed -i "s|/etc/$NEW_NAME/$OLD_NAME.env|/etc/$NEW_NAME/service.env|g" "gateway/systemd/$NEW_SERVICE.service"

# Go module update
echo ">>> Updating Go module..."
cd gateway
if [ -f "go.mod" ]; then
    go mod edit -module github.com/prabhanshu/$NEW_NAME
fi

echo ">>> Rebuilding binary..."
go build -o $NEW_SERVICE ./cmd/server
cd ..

echo ">>> Installing new service..."
sudo cp "gateway/systemd/$NEW_SERVICE.service" /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now $NEW_SERVICE

echo ">>> DONE!"
echo "New service name: $NEW_SERVICE"
echo "New config path: /etc/$NEW_NAME/service.env"
echo "Please rename the parent folder manually if desired: mv $PROJECT_DIR $NEW_NAME"
