#!/usr/bin/env bash
# Create a new OSD profiler test session directory

set -e

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEMPLATE="$SCRIPT_DIR/session-template.md"

# Check if description provided
if [ $# -eq 0 ]; then
    echo "Usage: $0 <session-description>"
    echo ""
    echo "Examples:"
    echo "  $0 baseline-test"
    echo "  $0 long-recording-bug"
    echo "  $0 rapid-fire-testing"
    exit 1
fi

# Get description and sanitize it (replace spaces with dashes, lowercase)
DESCRIPTION=$(echo "$1" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | sed 's/[^a-z0-9-]//g')

# Generate session ID with timestamp
TIMESTAMP=$(date +%Y%m%d-%H%M)
SESSION_ID="${TIMESTAMP}-${DESCRIPTION}"
SESSION_DIR="$SCRIPT_DIR/$SESSION_ID"

# Check if session already exists
if [ -d "$SESSION_DIR" ]; then
    echo "Error: Session directory already exists: $SESSION_DIR"
    exit 1
fi

# Create session directory structure
echo "Creating test session: $SESSION_ID"
mkdir -p "$SESSION_DIR"
mkdir -p "$SESSION_DIR/screenshots"
mkdir -p "$SESSION_DIR/recordings"

# Copy template and fill in basic info
NOTES_FILE="$SESSION_DIR/notes.md"
cp "$TEMPLATE" "$NOTES_FILE"

# Replace placeholders
CURRENT_DATE=$(date +"%Y-%m-%d %H:%M")
sed -i "s/YYYY-MM-DD HH:MM/$CURRENT_DATE/g" "$NOTES_FILE"
sed -i "s/YYYYMMDD-HHMM-description/$SESSION_ID/g" "$NOTES_FILE"
sed -i "s/\[DESCRIPTION\]/$DESCRIPTION/g" "$NOTES_FILE"

# Create placeholder for profiler output
touch "$SESSION_DIR/profiler-output.txt"

# Create session README
cat > "$SESSION_DIR/README.md" <<EOF
# Session: $SESSION_ID

Created: $CURRENT_DATE

## Quick Start

1. **Start profiler with output capture**:
   \`\`\`bash
   cd ~/Programs/omarchy-voice-typing/tools
   python osd_profiler.py | tee ../test-sessions/$SESSION_ID/profiler-output.txt
   \`\`\`

2. **Test voice typing** (Super+\` in another window)

3. **Document findings** in \`notes.md\`

4. **Stop profiler** with Ctrl+C when done

5. **Update** \`../INDEX.md\` with key findings

## Files

- \`notes.md\` - Test observations and analysis (use the template)
- \`profiler-output.txt\` - Captured profiler output
- \`screenshots/\` - Screenshots of OSD behavior
- \`recordings/\` - Audio recordings (optional)
EOF

# Success message
echo ""
echo "✓ Test session created: $SESSION_DIR"
echo ""
echo "Next steps:"
echo "  1. cd $SESSION_DIR"
echo "  2. Review README.md for quick start"
echo "  3. Edit notes.md to document your test"
echo ""
echo "To start profiler:"
echo "  cd ~/Programs/omarchy-voice-typing/tools"
echo "  python osd_profiler.py | tee ../test-sessions/$SESSION_ID/profiler-output.txt"
echo ""
