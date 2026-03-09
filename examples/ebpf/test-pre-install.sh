#!/bin/bash
# Test runner for pre-install-complete.sh
# Tests the script across multiple Linux distributions using Docker

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT_TO_TEST="$SCRIPT_DIR/pre-install-complete.sh"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

print_header() {
    echo ""
    echo "========================================"
    echo "$1"
    echo "========================================"
    echo ""
}

print_result() {
    local status=$1
    local distro=$2

    if [ $status -eq 0 ]; then
        echo -e "${GREEN}✅ PASS${NC}: $distro"
    else
        echo -e "${RED}❌ FAIL${NC}: $distro (exit code: $status)"
    fi
}

# Detect host architecture
HOST_ARCH=$(uname -m)

# Test distributions
DISTROS=(
    "ubuntu:22.04:apt"
    "ubuntu:20.04:apt"
    "debian:11:apt"
    "fedora:38:dnf"
    "fedora:37:dnf"
    "rockylinux:9:dnf"
    "alpine:latest:apk"
)

# Add Arch Linux only on x86_64 systems (not available natively on ARM64)
if [[ "$HOST_ARCH" == "x86_64" ]]; then
    DISTROS+=("archlinux:latest:pacman")
fi

print_header "Testing pre-install-complete.sh across Linux distributions"

echo "Script: $SCRIPT_TO_TEST"
echo ""

# Check if Docker is available
if ! command -v docker &> /dev/null; then
    echo -e "${RED}Error: Docker is not installed or not in PATH${NC}"
    exit 1
fi

# Check if script exists
if [ ! -f "$SCRIPT_TO_TEST" ]; then
    echo -e "${RED}Error: Script not found: $SCRIPT_TO_TEST${NC}"
    exit 1
fi

# Results tracking
PASSED=0
FAILED=0
TOTAL=${#DISTROS[@]}

for distro_info in "${DISTROS[@]}"; do
    IFS=':' read -r image tag pkg_mgr <<< "$distro_info"
    distro_name="$image:$tag"

    echo ""
    echo "Testing: $distro_name (Package Manager: $pkg_mgr)"
    echo "----------------------------------------"

    # Shell detection
    initial_shell="bash"

    # Alpine uses sh initially, but we'll install bash
    if [[ "$image" == "alpine" ]]; then
        initial_shell="sh"
    fi

    # Create test script that will run inside container
    test_cmd="
        set -e

        # Install prerequisites
        if command -v apt-get &> /dev/null; then
            apt-get update -qq 2>&1 > /dev/null || true
            apt-get install -y -qq sudo procps 2>&1 > /dev/null || true
        elif command -v dnf &> /dev/null; then
            dnf install -y -q sudo which procps-ng 2>&1 > /dev/null || true
        elif command -v yum &> /dev/null; then
            yum install -y -q sudo which procps 2>&1 > /dev/null || true
        elif command -v pacman &> /dev/null; then
            pacman -Sy --noconfirm sudo which procps 2>&1 > /dev/null || true
        elif command -v apk &> /dev/null; then
            apk add --quiet bash sudo procps 2>&1 > /dev/null || true
        fi

        # Test detection functions only (no actual installation)
        # Always use bash since the script requires it
        echo 'Testing detection functions...'
        bash /scripts/pre-install-complete.sh 2>&1 | head -20 || true
    "

    # Run container with the script
    if docker run --rm \
        -v "$SCRIPT_DIR:/scripts:ro" \
        "$distro_name" \
        $initial_shell -c "$test_cmd" > /tmp/test-output-$pkg_mgr.log 2>&1; then

        # Check if detection worked
        if grep -q "Distribution:" /tmp/test-output-$pkg_mgr.log && \
           grep -q "Package manager:" /tmp/test-output-$pkg_mgr.log; then
            print_result 0 "$distro_name"
            PASSED=$((PASSED + 1))
        else
            print_result 1 "$distro_name"
            FAILED=$((FAILED + 1))
            echo "  Reason: Detection functions failed"
        fi
    else
        print_result 1 "$distro_name"
        FAILED=$((FAILED + 1))
        echo "  Reason: Docker run failed"
    fi

    # Show relevant output
    if [ -f /tmp/test-output-$pkg_mgr.log ]; then
        echo "  Output preview:"
        grep -E "(Distribution|Package manager|Kernel version|Current kernel)" /tmp/test-output-$pkg_mgr.log | sed 's/^/    /' || true
    fi
done

# Summary
print_header "Test Summary"
echo "Total distributions tested: $TOTAL"
echo -e "${GREEN}Passed: $PASSED${NC}"
echo -e "${RED}Failed: $FAILED${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All tests passed! ✅${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed ❌${NC}"
    exit 1
fi
