#!/bin/bash
# Test runner for post-install.sh
# Creates a simulated package folder and tests the post-install script

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
POST_INSTALL_SCRIPT="$SCRIPT_DIR/post-install.sh"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Test configuration
TEST_PACKAGE_FOLDER="/tmp/ebpf-test-package-$$"
SYSTEM_LIB_PATH="/lib/newrelic-ebpf-agent"
BINARY_NAME="nr-ebpf-agent"

print_header() {
    echo ""
    echo "========================================"
    echo "$1"
    echo "========================================"
    echo ""
}

print_step() {
    echo -e "${BLUE}▶ $1${NC}"
}

print_ok() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

print_info() {
    echo -e "${YELLOW}ℹ️  $1${NC}"
}

cleanup() {
    print_step "Cleaning up test environment..."

    # Remove test package folder
    if [ -d "$TEST_PACKAGE_FOLDER" ]; then
        rm -rf "$TEST_PACKAGE_FOLDER"
        print_ok "Removed test package folder"
    fi

    # Remove system installation (requires sudo)
    if [ -d "$SYSTEM_LIB_PATH" ]; then
        sudo rm -rf "$SYSTEM_LIB_PATH"
        print_ok "Removed system installation"
    fi

    echo ""
}

# Trap to ensure cleanup on exit
trap cleanup EXIT

print_header "Testing post-install.sh"

print_info "Test package folder: $TEST_PACKAGE_FOLDER"
print_info "System lib path: $SYSTEM_LIB_PATH"
echo ""

# ============================================================================
# PRE-TEST CHECKS
# ============================================================================

print_step "Running pre-test checks..."

# Check if script exists
if [ ! -f "$POST_INSTALL_SCRIPT" ]; then
    print_error "Script not found: $POST_INSTALL_SCRIPT"
    exit 1
fi
print_ok "Post-install script found"

# Check if running with appropriate permissions
if [ "$EUID" -eq 0 ]; then
    print_error "Do not run this test as root. The script will request sudo when needed."
    exit 1
fi
print_ok "Running as non-root user"

# Check sudo access
if ! sudo -n true 2>/dev/null; then
    print_info "This test requires sudo access. You may be prompted for your password."
    sudo -v
fi
print_ok "Sudo access confirmed"

echo ""

# ============================================================================
# TEST SETUP
# ============================================================================

print_step "Setting up test environment..."

# Create simulated package folder structure
mkdir -p "$TEST_PACKAGE_FOLDER/lib/newrelic-ebpf-agent"
print_ok "Created package folder structure"

# Create a fake eBPF agent binary
cat > "$TEST_PACKAGE_FOLDER/lib/newrelic-ebpf-agent/$BINARY_NAME" << 'EOF'
#!/bin/bash
echo "New Relic eBPF Agent (Test Binary)"
echo "Version: 0.1.0-test"
exit 0
EOF
chmod +x "$TEST_PACKAGE_FOLDER/lib/newrelic-ebpf-agent/$BINARY_NAME"
print_ok "Created fake eBPF agent binary"

# Create some additional fake shared libraries
touch "$TEST_PACKAGE_FOLDER/lib/newrelic-ebpf-agent/libebpf.so"
touch "$TEST_PACKAGE_FOLDER/lib/newrelic-ebpf-agent/libprobe.so"
print_ok "Created fake shared libraries"

# Create a config file
cat > "$TEST_PACKAGE_FOLDER/lib/newrelic-ebpf-agent/config.yaml" << 'EOF'
agent:
  name: ebpf-agent
  version: 0.1.0-test
EOF
print_ok "Created fake config file"

echo ""
print_info "Package folder contents:"
find "$TEST_PACKAGE_FOLDER" -type f -exec ls -lh {} \; | sed 's/^/  /'
echo ""

# ============================================================================
# RUN POST-INSTALL SCRIPT
# ============================================================================

print_step "Running post-install.sh..."
echo ""

# Change to the package folder (as the script expects)
cd "$TEST_PACKAGE_FOLDER"

# Run the post-install script
if bash "$POST_INSTALL_SCRIPT"; then
    echo ""
    print_ok "Post-install script completed successfully"
else
    echo ""
    print_error "Post-install script failed"
    exit 1
fi

echo ""

# ============================================================================
# VERIFICATION
# ============================================================================

print_step "Verifying installation..."
echo ""

FAILED=0

# 1. Check system lib path exists
if [ -d "$SYSTEM_LIB_PATH" ]; then
    print_ok "System lib path exists: $SYSTEM_LIB_PATH"
else
    print_error "System lib path not found: $SYSTEM_LIB_PATH"
    FAILED=$((FAILED + 1))
fi

# 2. Check binary exists in system path
if [ -f "$SYSTEM_LIB_PATH/$BINARY_NAME" ]; then
    print_ok "Binary found in system path"
else
    print_error "Binary not found in system path"
    FAILED=$((FAILED + 1))
fi

# 3. Check binary is executable
if [ -x "$SYSTEM_LIB_PATH/$BINARY_NAME" ]; then
    print_ok "Binary is executable"
else
    print_error "Binary is not executable"
    FAILED=$((FAILED + 1))
fi

# 4. Check symlink exists in package folder
if [ -L "$TEST_PACKAGE_FOLDER/$BINARY_NAME" ]; then
    print_ok "Symlink exists in package folder"
else
    print_error "Symlink not found in package folder"
    FAILED=$((FAILED + 1))
fi

# 5. Check symlink points to correct target
if [ -L "$TEST_PACKAGE_FOLDER/$BINARY_NAME" ]; then
    LINK_TARGET=$(readlink "$TEST_PACKAGE_FOLDER/$BINARY_NAME")
    EXPECTED_TARGET="$SYSTEM_LIB_PATH/$BINARY_NAME"
    if [ "$LINK_TARGET" = "$EXPECTED_TARGET" ]; then
        print_ok "Symlink points to correct target"
    else
        print_error "Symlink target mismatch"
        print_error "  Expected: $EXPECTED_TARGET"
        print_error "  Got: $LINK_TARGET"
        FAILED=$((FAILED + 1))
    fi
fi

# 6. Check symlink is valid (not broken)
if [ -e "$TEST_PACKAGE_FOLDER/$BINARY_NAME" ]; then
    print_ok "Symlink is valid (not broken)"
else
    print_error "Symlink is broken"
    FAILED=$((FAILED + 1))
fi

# 7. Check original lib folder was removed from package folder
if [ ! -d "$TEST_PACKAGE_FOLDER/lib/newrelic-ebpf-agent" ]; then
    print_ok "Original lib folder removed from package folder"
else
    print_error "Original lib folder still exists in package folder"
    FAILED=$((FAILED + 1))
fi

# 8. Check shared libraries moved to system path
if [ -f "$SYSTEM_LIB_PATH/libebpf.so" ] && [ -f "$SYSTEM_LIB_PATH/libprobe.so" ]; then
    print_ok "Shared libraries moved to system path"
else
    print_error "Shared libraries not found in system path"
    FAILED=$((FAILED + 1))
fi

# 9. Check config file moved to system path
if [ -f "$SYSTEM_LIB_PATH/config.yaml" ]; then
    print_ok "Config file moved to system path"
else
    print_error "Config file not found in system path"
    FAILED=$((FAILED + 1))
fi

# 10. Test executing the binary through symlink
echo ""
print_step "Testing binary execution through symlink..."
if "$TEST_PACKAGE_FOLDER/$BINARY_NAME" > /tmp/ebpf-test-output.log 2>&1; then
    if grep -q "New Relic eBPF Agent" /tmp/ebpf-test-output.log; then
        print_ok "Binary executes correctly through symlink"
        print_info "Output:"
        cat /tmp/ebpf-test-output.log | sed 's/^/    /'
    else
        print_error "Binary output unexpected"
        FAILED=$((FAILED + 1))
    fi
else
    print_error "Failed to execute binary through symlink"
    FAILED=$((FAILED + 1))
fi

echo ""

# Show final structure
print_info "Final system structure:"
echo "  Package folder:"
ls -la "$TEST_PACKAGE_FOLDER" | sed 's/^/    /'
echo ""
echo "  System lib path:"
sudo ls -la "$SYSTEM_LIB_PATH" | sed 's/^/    /'

echo ""

# ============================================================================
# SUMMARY
# ============================================================================

print_header "Test Summary"

if [ $FAILED -eq 0 ]; then
    print_ok "All tests passed! ✅"
    echo ""
    exit 0
else
    print_error "$FAILED test(s) failed ❌"
    echo ""
    exit 1
fi
