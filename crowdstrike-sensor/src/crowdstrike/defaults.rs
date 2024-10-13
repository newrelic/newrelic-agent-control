pub const CROWDSTRIKE_SENSOR_INSTALLER_HASH_OS_MAPPING: &str = r#"
mapping:
    ubuntu_debian_amd64_hash:
        os: "Debian"
        os_version: "9/10/11/12"
        name_pattern: ".*\\.amd64.deb$"
    ubuntu_arm64_hash:
        os: "Ubuntu"
        os_version: "18/20/22 - arm64"
        name_pattern: ".*\\_arm64.deb$"
    sles12_amd64_hash:
        os: "SLES"
        os_version: "12"
        name_pattern: ".*\\.x86_64.rpm$"
    sles15_amd64_hash:
        os: "SLES"
        os_version: "15"
        name_pattern: ".*\\.x86_64.rpm$"
    centos7_amd64_hash:
        os: "RHEL/CentOS/Oracle"
        os_version: "7"
        name_pattern: ".*\\.x86_64.rpm$"
    centos8_amd64_hash:
        os: "RHEL/CentOS/Oracle"
        os_version: "8"
        name_pattern: ".*\\.x86_64.rpm$"
    centos8_arm64_hash:
        os: "RHEL/CentOS"
        os_version: "8 - arm64"
        name_pattern: ".*\\.aarch64.rpm$"
    redhat7_amd64_hash:
        os: "RHEL/CentOS/Oracle"
        os_version: "7"
        name_pattern: ".*\\.x86_64.rpm$"
    redhat8_amd64_hash:
        os: "RHEL/CentOS/Oracle"
        os_version: "8"
        name_pattern: ".*\\.x86_64.rpm$"
    redhat9_amd64_hash:
        os: "RHEL/CentOS Stream/Oracle"
        os_version: "9"
        name_pattern: ".*\\.x86_64.rpm$"
    redhat9_arm64_hash:
        os: "RHEL"
        os_version: "9 - arm64"
        name_pattern: ".*\\.aarch64.rpm$"
    al2_amd64_hash:
        os: "Amazon Linux"
        os_version: "2"
        name_pattern: ".*\\.x86_64.rpm$"
    al2_arm64_hash:
        os: "Amazon Linux"
        os_version: "2 - arm64"
        name_pattern: ".*\\.aarch64.rpm$"
    al2023_amd64_hash:
        os: "Amazon Linux"
        os_version: "2023"
        name_pattern: ".*\\.x86_64.rpm$"
    al2023_arm64_hash:
        os: "Amazon Linux"
        os_version: "2023 - arm64"
        name_pattern: ".*\\.aarch64.rpm$"
    win_hash:
        os: "Windows"
        os_version: "Windows"
        name_pattern: ".*\\.exe$"
    mac_hash:
        os: "macOS"
        os_version: ""
        name_pattern: ".*\\.pkg$"
"#;