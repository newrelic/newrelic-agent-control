use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Token {
    pub(super) access_token: String,
    pub(super) expires_in: u32,
    pub(super) token_type: String,
}

#[derive(Deserialize)]
pub(super) struct Sensor {
    #[serde(rename = "resources")]
    pub(super) installers: Vec<Installer>
}
#[derive(Deserialize)]
pub(super) struct Installer {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) platform: String,
    pub(super) os: String,
    pub(super) os_version: String,
    pub(super) sha256: String,
    pub(super) release_date: String,
    pub(super) version: String,
    pub(super) file_size: u32,
    pub(super) file_type: String,
}

/*
    {
    "name": "falcon-sensor-7.16.0-16903.el7.x86_64.rpm",
    "description": "Falcon Kernel Sensor for RHEL/CentOS/Oracle 7",
    "platform": "linux",
    "os": "RHEL/CentOS/Oracle",
    "os_version": "7",
    "sha256": "67702a5edff9ca5cf01e503becfd4f1781a9f2c2658583a83eb63cc277f50a5e",
    "release_date": "2024-06-11T23:09:05.178Z",
    "version": "7.16.16903",
    "file_size": 67278396,
    "file_type": "rpm"
    },
*/