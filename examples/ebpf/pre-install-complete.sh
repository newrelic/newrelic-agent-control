#!/bin/bash
# Complete pre-install hook for eBPF agent
# This script does EVERYTHING: validation, detection, and installation

set -e

echo "==================================="
echo "eBPF Agent Pre-Install Validation"
echo "==================================="
echo ""

# ============================================================================
# CONFIGURATION
# ============================================================================

MIN_KERNEL_VERSION="4.14.0"
REQUIRED_PACKAGES=("linux-headers")  # Generic name, will be resolved per distro

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

# Compare version strings (returns 0 if v1 >= v2)
version_ge() {
    [ "$(printf '%s\n' "$1" "$2" | sort -V | head -n1)" = "$2" ]
}

# Print colored messages
print_ok() {
    echo "✅ $1"
}

print_warn() {
    echo "⚠️  $1"
}

print_error() {
    echo "❌ $1"
}

print_info() {
    echo "ℹ️  $1"
}

# ============================================================================
# SYSTEM DETECTION
# ============================================================================

detect_distro() {
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        echo "$ID"
    else
        echo "unknown"
    fi
}

detect_package_manager() {
    if command -v apt-get &> /dev/null; then
        echo "apt"
    elif command -v dnf &> /dev/null; then
        echo "dnf"
    elif command -v yum &> /dev/null; then
        echo "yum"
    elif command -v pacman &> /dev/null; then
        echo "pacman"
    elif command -v zypper &> /dev/null; then
        echo "zypper"
    elif command -v apk &> /dev/null; then
        echo "apk"
    else
        echo "unknown"
    fi
}

get_kernel_version() {
    uname -r
}

get_kernel_major_minor() {
    uname -r | cut -d'-' -f1
}

# ============================================================================
# PACKAGE MANAGER OPERATIONS
# ============================================================================

# Get the correct package name for linux-headers based on distro
get_headers_package_name() {
    local pkg_mgr=$1
    local kernel_version=$2

    case $pkg_mgr in
        apt)
            echo "linux-headers-$kernel_version"
            ;;
        yum|dnf)
            echo "kernel-devel-$kernel_version"
            ;;
        pacman)
            echo "linux-headers"
            ;;
        zypper)
            echo "kernel-default-devel-$kernel_version"
            ;;
        apk)
            echo "linux-headers"
            ;;
        *)
            echo "linux-headers-$kernel_version"
            ;;
    esac
}

is_package_installed() {
    local pkg_mgr=$1
    local package=$2

    case $pkg_mgr in
        apt)
            dpkg -l "$package" 2>/dev/null | grep -q "^ii"
            ;;
        yum|dnf)
            rpm -q "$package" &> /dev/null
            ;;
        pacman)
            pacman -Q "$package" &> /dev/null
            ;;
        zypper)
            rpm -q "$package" &> /dev/null
            ;;
        apk)
            apk info -e "$package" &> /dev/null
            ;;
        *)
            return 1
            ;;
    esac
}

is_package_available() {
    local pkg_mgr=$1
    local package=$2

    case $pkg_mgr in
        apt)
            apt-cache search "^$package\$" 2>/dev/null | grep -q "."
            ;;
        yum)
            yum search "$package" 2>/dev/null | grep -q "$package"
            ;;
        dnf)
            dnf search "$package" 2>/dev/null | grep -q "$package"
            ;;
        pacman)
            pacman -Ss "^$package\$" 2>/dev/null | grep -q "."
            ;;
        zypper)
            zypper search -x "$package" 2>/dev/null | grep -q "$package"
            ;;
        apk)
            apk search "$package" 2>/dev/null | grep -q "."
            ;;
        *)
            return 1
            ;;
    esac
}

install_package() {
    local pkg_mgr=$1
    local package=$2

    print_info "Package: $package"
    print_info "Package Manager: $pkg_mgr"
    echo ""
    print_info "Installing $package..."

    case $pkg_mgr in
        apt)
            sudo apt-get update -qq && sudo apt-get install -y -qq "$package"
            ;;
        yum)
            sudo yum install -y -q "$package"
            ;;
        dnf)
            sudo dnf install -y -q "$package"
            ;;
        pacman)
            sudo pacman -S --noconfirm "$package"
            ;;
        zypper)
            sudo zypper install -y "$package"
            ;;
        apk)
            sudo apk add --quiet "$package"
            ;;
        *)
            print_error "Unknown package manager: $pkg_mgr"
            return 1
            ;;
    esac

    if [ $? -eq 0 ]; then
        print_ok "Package $package installed successfully"
        return 0
    else
        print_error "Failed to install $package"
        return 1
    fi
}

# ============================================================================
# VALIDATION FUNCTIONS
# ============================================================================

validate_kernel_version() {
    echo "Checking kernel version..."

    local kernel_version=$(get_kernel_version)
    local kernel_major_minor=$(get_kernel_major_minor)

    print_info "Current kernel: $kernel_version"
    print_info "Required minimum: $MIN_KERNEL_VERSION"

    if version_ge "$kernel_major_minor" "$MIN_KERNEL_VERSION"; then
        print_ok "Kernel version meets requirements"
        echo ""
        return 0
    else
        print_error "Kernel version $kernel_version is below minimum $MIN_KERNEL_VERSION"
        print_error "eBPF requires kernel >= $MIN_KERNEL_VERSION"
        echo ""
        return 1
    fi
}

validate_and_install_dependencies() {
    echo "Checking system dependencies..."

    # Detect distro and package manager
    local distro=$(detect_distro)
    local pkg_mgr=$(detect_package_manager)

    print_info "Distribution: $distro"
    print_info "Package manager: $pkg_mgr"
    echo ""

    if [ "$pkg_mgr" = "unknown" ]; then
        print_error "Could not detect package manager"
        print_error "Supported: apt, yum, dnf, pacman, zypper, apk"
        echo ""
        return 1
    fi

    # Check each required package
    local kernel_version=$(get_kernel_version)

    for generic_pkg in "${REQUIRED_PACKAGES[@]}"; do
        # Resolve generic name to distro-specific package
        local actual_pkg
        if [ "$generic_pkg" = "linux-headers" ]; then
            actual_pkg=$(get_headers_package_name "$pkg_mgr" "$kernel_version")
        else
            actual_pkg=$generic_pkg
        fi

        echo "Checking dependency: $generic_pkg"
        print_info "Resolved to: $actual_pkg"

        # Check if already installed
        if is_package_installed "$pkg_mgr" "$actual_pkg"; then
            print_ok "Already installed: $actual_pkg"
            echo ""
            continue
        fi

        # Not installed - check if available
        print_warn "Not installed: $actual_pkg"

        if ! is_package_available "$pkg_mgr" "$actual_pkg"; then
            print_error "Package not available in repositories"
            print_error "Your kernel version may be too old or EOL"
            echo ""
            print_info "Possible solutions:"
            echo "  1. Upgrade your kernel to a supported version"
            echo "  2. Enable additional repositories"
            echo "  3. Manually install kernel headers"
            echo ""
            return 1
        fi

        # Available but not installed - prompt to install
        echo ""
        if ! install_package "$pkg_mgr" "$actual_pkg"; then
            return 1
        fi
        echo ""
    done

    print_ok "All dependencies satisfied"
    echo ""
    return 0
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    # 1. Validate kernel version
    if ! validate_kernel_version; then
        exit 1
    fi

    # 2. Validate and install dependencies
    if ! validate_and_install_dependencies; then
        exit 1
    fi

    # 3. All checks passed
    echo "==================================="
    print_ok "Pre-install validation complete!"
    echo "==================================="
    echo ""

    exit 0
}

# Run main
main
