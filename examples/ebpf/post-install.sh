#!/bin/bash
# eBPF Agent Post-Install Script
#
# This script performs system integration after the eBPF agent OCI package
# has been extracted to the local package folder.

set -e

# ============================================================================
# CONFIGURATION
# ============================================================================

# System installation path for eBPF agent
SYSTEM_LIB_PATH="/lib/newrelic-ebpf-agent"

# Binary name
BINARY_NAME="nr-ebpf-agent"

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

print_ok() {
    echo "✅ $1"
}

print_info() {
    echo "ℹ️  $1"
}

print_error() {
    echo "❌ ERROR: $1" >&2
}

# ============================================================================
# VALIDATION
# ============================================================================


PACKAGE_FOLDER=$(pwd)
SOURCE_LIB_PATH="$PACKAGE_FOLDER/lib/newrelic-ebpf-agent"
SYMLINK_PATH="$PACKAGE_FOLDER/$BINARY_NAME"

echo ""
echo "==================================="
echo "eBPF Agent Post-Install"
echo "==================================="
echo ""

print_info "Package folder: $PACKAGE_FOLDER"
print_info "Source path: $SOURCE_LIB_PATH"
print_info "Target path: $SYSTEM_LIB_PATH"
echo ""

# Check if source directory exists
if [ ! -d "$SOURCE_LIB_PATH" ]; then
    print_error "Source directory not found: $SOURCE_LIB_PATH"
    print_error "Expected structure: [package_folder]/lib/newrelic-ebpf-agent/"
    exit 1
fi

print_ok "Source directory found"

# Check if binary exists in source
if [ ! -f "$SOURCE_LIB_PATH/$BINARY_NAME" ]; then
    print_error "Binary not found: $SOURCE_LIB_PATH/$BINARY_NAME"
    exit 1
fi

print_ok "Binary found: $BINARY_NAME"
echo ""

# ============================================================================
# SYSTEM INTEGRATION
# ============================================================================

echo "Starting system integration..."
echo ""

# 1. Move assets to system library path
print_info "Moving assets to $SYSTEM_LIB_PATH..."

# Remove existing installation if present
if [ -d "$SYSTEM_LIB_PATH" ]; then
    print_info "Removing existing installation..."
    sudo rm -rf "$SYSTEM_LIB_PATH"
fi

# Create parent directory if needed
sudo mkdir -p "$(dirname "$SYSTEM_LIB_PATH")"

# Move the lib folder to system path
sudo mv "$SOURCE_LIB_PATH" "$SYSTEM_LIB_PATH"

if [ $? -eq 0 ]; then
    print_ok "Assets moved successfully"
else
    print_error "Failed to move assets to system path"
    exit 1
fi

echo ""

# 2. Create symbolic link in package folder
print_info "Creating symbolic link..."

# Remove existing symlink if present
if [ -L "$SYMLINK_PATH" ] || [ -f "$SYMLINK_PATH" ]; then
    rm -f "$SYMLINK_PATH"
fi

# Create symlink pointing to system binary
ln -s "$SYSTEM_LIB_PATH/$BINARY_NAME" "$SYMLINK_PATH"

if [ $? -eq 0 ]; then
    print_ok "Symbolic link created: $SYMLINK_PATH -> $SYSTEM_LIB_PATH/$BINARY_NAME"
else
    print_error "Failed to create symbolic link"
    exit 1
fi

echo ""

# ============================================================================
# VERIFICATION
# ============================================================================

echo "Verifying installation..."
echo ""

# Check system path exists
if [ ! -d "$SYSTEM_LIB_PATH" ]; then
    print_error "System path not found after installation"
    exit 1
fi

print_ok "System path verified: $SYSTEM_LIB_PATH"

# Check binary exists in system path
if [ ! -f "$SYSTEM_LIB_PATH/$BINARY_NAME" ]; then
    print_error "Binary not found in system path"
    exit 1
fi

print_ok "Binary verified: $SYSTEM_LIB_PATH/$BINARY_NAME"

# Check symlink is valid
if [ ! -L "$SYMLINK_PATH" ]; then
    print_error "Symbolic link not found"
    exit 1
fi

if [ ! -e "$SYMLINK_PATH" ]; then
    print_error "Symbolic link is broken"
    exit 1
fi

print_ok "Symbolic link verified: $SYMLINK_PATH"

# Check binary is executable
if [ ! -x "$SYSTEM_LIB_PATH/$BINARY_NAME" ]; then
    print_info "Setting executable permissions on binary..."
    sudo chmod +x "$SYSTEM_LIB_PATH/$BINARY_NAME"
fi

print_ok "Binary is executable"

echo ""
echo "==================================="
echo "Post-Install Complete"
echo "==================================="
echo ""
print_info "eBPF agent installed at: $SYSTEM_LIB_PATH"
print_info "Entry point: $SYMLINK_PATH"
echo ""

exit 0
