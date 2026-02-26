#!/usr/bin/env bash
set -e # Exit immediately if a command exits with a non-zero status

JSON_FILE=$1
VERSION=$2
EXECUTABLE=$3
CURRENT_YEAR=$(date +'%Y')
TMP_JSON="${JSON_FILE}.tmp"

echo "Patching metadata for $EXECUTABLE with year $CURRENT_YEAR..."

# 1. Replace the placeholder with the current year and create a temporary file
sed "s/<CURRENT_YEAR>/$CURRENT_YEAR/g" "$JSON_FILE" > "$TMP_JSON"

# 2. Inject the metadata using go-winres
/home/runner/go/bin/go-winres patch --no-backup --in "$TMP_JSON" --product-version "$VERSION" --file-version "$VERSION" "$EXECUTABLE"

# 3. Clean up the temporary file
rm "$TMP_JSON"