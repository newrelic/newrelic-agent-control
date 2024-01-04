//! Azure EC2 instance id detector implementation
use std::time::Duration;

use thiserror::Error;

use crate::cloud::http_client::{HttpClient, HttpClientError, HttpClientUreq};
use crate::{cloud::AZURE_INSTANCE_ID, Detect, DetectError, Key, Resource, Value};

use super::metadata::{AzureMetadata, IPV4_METADATA_ENDPOINT};

/// The `AzureDetector` struct encapsulates an HTTP client used to retrieve the instance metadata.

pub struct AzureDetector<C: HttpClient> {
    http_client: C,
}

const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

impl Default for AzureDetector<HttpClientUreq> {
    fn default() -> Self {
        Self {
            http_client: HttpClientUreq::new(
                IPV4_METADATA_ENDPOINT.to_string(),
                DEFAULT_CLIENT_TIMEOUT,
            ),
        }
    }
}

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum AzureDetectorError {
    /// Internal HTTP error
    #[error("`{0}`")]
    HttpError(#[from] HttpClientError),
    /// Error while deserializing endpoint metadata
    #[error("error deserializing json: `{0}`")]
    JsonError(#[from] serde_json::Error),
    /// Unsuccessful HTTP response.
    #[error("Status code: `{0}` Canonical reason: `{1}`")]
    UnsuccessfulResponse(u16, String),
}

impl<C> Detect for AzureDetector<C>
where
    C: HttpClient,
{
    fn detect(&self) -> Result<Resource, DetectError> {
        let response = self
            .http_client
            .get()
            .map_err(AzureDetectorError::HttpError)?;

        // return error if status code is not within 200-299.
        if !response.status().is_success() {
            return Err(DetectError::AzureError(
                AzureDetectorError::UnsuccessfulResponse(
                    response.status().as_u16(),
                    response
                        .status()
                        .canonical_reason()
                        .unwrap_or_default()
                        .to_string(),
                ),
            ));
        }

        let metadata: AzureMetadata =
            serde_json::from_slice(response.body()).map_err(AzureDetectorError::JsonError)?;

        Ok(Resource::new([(
            Key::from(AZURE_INSTANCE_ID),
            Value::from(metadata.compute.instance_id),
        )]))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cloud::http_client::test::MockHttpClientMock;
    use http::Response;

    #[test]
    fn http_client_error() {
        let mut client_mock = MockHttpClientMock::new();
        let error = HttpClientError::UreqError(String::from("some error"));
        client_mock.should_not_get(error);

        let detector = AzureDetector {
            http_client: client_mock,
        };

        let detect_error = detector.detect().unwrap_err();
        assert_eq!(
            "error detecting azure resources ``internal HTTP client error: `some error```",
            detect_error.to_string()
        );
    }

    #[test]
    fn invalid_response_code() {
        let mut client_mock = MockHttpClientMock::new();
        client_mock.should_get(Response::from(
            http::Response::builder()
                .status(503)
                .body("".as_bytes().to_vec())
                .unwrap(),
        ));

        let detector = AzureDetector {
            http_client: client_mock,
        };

        let detect_error = detector.detect().unwrap_err();
        assert_eq!(
            "error detecting azure resources `Status code: `503` Canonical reason: `Service Unavailable``",
            detect_error.to_string()
        );
    }

    #[test]
    fn error_on_deserializing() {
        let mut client_mock = MockHttpClientMock::new();
        client_mock.should_get(Response::from(
            http::Response::builder()
                .status(200)
                .body("bad:json".as_bytes().to_vec())
                .unwrap(),
        ));

        let detector = AzureDetector {
            http_client: client_mock,
        };

        let detect_error = detector.detect().unwrap_err();
        assert_eq!(
            "error detecting azure resources `error deserializing json: `expected value at line 1 column 1``",
            detect_error.to_string()
        );
    }

    #[test]
    fn detect_azure_metadata_from_windows_vm() {
        // https://learn.microsoft.com/en-us/azure/virtual-machines/instance-metadata-service?tabs=windows
        let mut client_mock = MockHttpClientMock::new();
        client_mock.should_get(Response::from(
            http::Response::builder()
                .status(200)
                .body(WINDOWS_VM_RESPONSE.as_bytes().to_vec())
                .unwrap(),
        ));

        let detector = AzureDetector {
            http_client: client_mock,
        };

        let identifiers = detector.detect().unwrap();

        assert_eq!(
            "02aab8a4-74ef-476e-8182-f6d2ba4166a6".to_string(),
            String::from(identifiers.get(AZURE_INSTANCE_ID.into()).unwrap())
        )
    }

    #[test]
    fn detect_azure_metadata_from_linux_vm() {
        // https://learn.microsoft.com/en-us/azure/virtual-machines/instance-metadata-service?tabs=linux
        let mut client_mock = MockHttpClientMock::new();
        client_mock.should_get(Response::from(
            http::Response::builder()
                .status(200)
                .body(LINUX_VM_RESPONSE.as_bytes().to_vec())
                .unwrap(),
        ));

        let detector = AzureDetector {
            http_client: client_mock,
        };

        let identifiers = detector.detect().unwrap();

        assert_eq!(
            "02aab8a4-74ef-476e-8182-f6d2ba4166a7".to_string(),
            String::from(identifiers.get(AZURE_INSTANCE_ID.into()).unwrap())
        )
    }

    const LINUX_VM_RESPONSE: &'static str = r#"
    {
    "compute": {
        "azEnvironment": "AZUREPUBLICCLOUD",
        "additionalCapabilities": {
            "hibernationEnabled": "true"
        },
        "hostGroup": {
          "id": "testHostGroupId"
        }, 
        "extendedLocation": {
            "type": "edgeZone",
            "name": "microsoftlosangeles"
        },
        "evictionPolicy": "",
        "isHostCompatibilityLayerVm": "true",
        "licenseType":  "",
        "location": "westus",
        "name": "examplevmname",
        "offer": "UbuntuServer",
        "osProfile": {
            "adminUsername": "admin",
            "computerName": "examplevmname",
            "disablePasswordAuthentication": "true"
        },
        "osType": "Linux",
        "placementGroupId": "f67c14ab-e92c-408c-ae2d-da15866ec79a",
        "plan": {
            "name": "planName",
            "product": "planProduct",
            "publisher": "planPublisher"
        },
        "platformFaultDomain": "36",
        "platformSubFaultDomain": "",        
        "platformUpdateDomain": "42",
        "priority": "Regular",
        "publicKeys": [{
                "keyData": "ssh-rsa 0",
                "path": "/home/user/.ssh/authorized_keys0"
            },
            {
                "keyData": "ssh-rsa 1",
                "path": "/home/user/.ssh/authorized_keys1"
            }
        ],
        "publisher": "Canonical",
        "resourceGroupName": "macikgo-test-may-23",
        "resourceId": "/subscriptions/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx/resourceGroups/macikgo-test-may-23/providers/Microsoft.Compute/virtualMachines/examplevmname",
        "securityProfile": {
            "secureBootEnabled": "true",
            "virtualTpmEnabled": "false",
            "encryptionAtHost": "true",
            "securityType": "TrustedLaunch"
        },
        "sku": "18.04-LTS",
        "storageProfile": {
            "dataDisks": [{
                "bytesPerSecondThrottle": "979202048",
                "caching": "None",
                "createOption": "Empty",
                "diskCapacityBytes": "274877906944",
                "diskSizeGB": "1024",
                "image": {
                  "uri": ""
                },
                "isSharedDisk": "false",
                "isUltraDisk": "true",
                "lun": "0",
                "managedDisk": {
                  "id": "/subscriptions/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx/resourceGroups/macikgo-test-may-23/providers/Microsoft.Compute/disks/exampledatadiskname",
                  "storageAccountType": "StandardSSD_LRS"
                },
                "name": "exampledatadiskname",
                "opsPerSecondThrottle": "65280",
                "vhd": {
                  "uri": ""
                },
                "writeAcceleratorEnabled": "false"
            }],
            "imageReference": {
                "id": "",
                "offer": "UbuntuServer",
                "publisher": "Canonical",
                "sku": "16.04.0-LTS",
                "version": "latest",
                "communityGalleryImageId": "/CommunityGalleries/testgallery/Images/1804Gen2/Versions/latest",
                "sharedGalleryImageId": "/SharedGalleries/1P/Images/gen2/Versions/latest",
                "exactVersion": "1.1686127202.30113"
            },
            "osDisk": {
                "caching": "ReadWrite",
                "createOption": "FromImage",
                "diskSizeGB": "30",
                "diffDiskSettings": {
                    "option": "Local"
                },
                "encryptionSettings": {
                  "enabled": "false",
                  "diskEncryptionKey": {
                    "sourceVault": {
                      "id": "/subscriptions/test-source-guid/resourceGroups/testrg/providers/Microsoft.KeyVault/vaults/test-kv"
                    },
                    "secretUrl": "https://test-disk.vault.azure.net/secrets/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx"
                  },
                  "keyEncryptionKey": {
                    "sourceVault": {
                      "id": "/subscriptions/test-key-guid/resourceGroups/testrg/providers/Microsoft.KeyVault/vaults/test-kv"
                    },
                    "keyUrl": "https://test-key.vault.azure.net/secrets/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx"
                  }
                },
                "image": {
                    "uri": ""
                },
                "managedDisk": {
                    "id": "/subscriptions/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx/resourceGroups/macikgo-test-may-23/providers/Microsoft.Compute/disks/exampleosdiskname",
                    "storageAccountType": "StandardSSD_LRS"
                },
                "name": "exampleosdiskname",
                "osType": "Linux",
                "vhd": {
                    "uri": ""
                },
                "writeAcceleratorEnabled": "false"
            },
            "resourceDisk": {
                "size": "4096"
            }
        },
        "subscriptionId": "xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx",
        "tags": "baz:bash;foo:bar",
        "version": "15.05.22",
        "virtualMachineScaleSet": {
            "id": "/subscriptions/xxxxxxxx-xxxxx-xxx-xxx-xxxx/resourceGroups/resource-group-name/providers/Microsoft.Compute/virtualMachineScaleSets/virtual-machine-scale-set-name"
        },
        "vmId": "02aab8a4-74ef-476e-8182-f6d2ba4166a7",
        "vmScaleSetName": "crpteste9vflji9",
        "vmSize": "Standard_A3",
        "zone": ""
    },
    "network": {
        "interface": [{
            "ipv4": {
               "ipAddress": [{
                    "privateIpAddress": "10.144.133.132",
                    "publicIpAddress": ""
                }],
                "subnet": [{
                    "address": "10.144.133.128",
                    "prefix": "26"
                }]
            },
            "ipv6": {
                "ipAddress": [
                 ]
            },
            "macAddress": "0011AAFFBB22"
        }]
    }
}
"#;

    const WINDOWS_VM_RESPONSE: &'static str = r#"
  {
    "compute": {
        "azEnvironment": "AZUREPUBLICCLOUD",
        "additionalCapabilities": {
            "hibernationEnabled": "true"
        },
        "hostGroup": {
          "id": "testHostGroupId"
        },
        "extendedLocation": {
            "type": "edgeZone",
            "name": "microsoftlosangeles"
        },
        "evictionPolicy": "",
        "isHostCompatibilityLayerVm": "true",
        "licenseType":  "Windows_Client",
        "location": "westus",
        "name": "examplevmname",
        "offer": "WindowsServer",
        "osProfile": {
            "adminUsername": "admin",
            "computerName": "examplevmname",
            "disablePasswordAuthentication": "true"
        },
        "osType": "Windows",
        "placementGroupId": "f67c14ab-e92c-408c-ae2d-da15866ec79a",
        "plan": {
            "name": "planName",
            "product": "planProduct",
            "publisher": "planPublisher"
        },
        "platformFaultDomain": "36",
        "platformSubFaultDomain": "",
        "platformUpdateDomain": "42",
        "priority": "Regular",
        "publicKeys": [{
                "keyData": "ssh-rsa 0",
                "path": "/home/user/.ssh/authorized_keys0"
            },
            {
                "keyData": "ssh-rsa 1",
                "path": "/home/user/.ssh/authorized_keys1"
            }
        ],
        "publisher": "RDFE-Test-Microsoft-Windows-Server-Group",
        "resourceGroupName": "macikgo-test-may-23",
        "resourceId": "/subscriptions/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx/resourceGroups/macikgo-test-may-23/providers/Microsoft.Compute/virtualMachines/examplevmname",
        "securityProfile": {
            "secureBootEnabled": "true",
            "virtualTpmEnabled": "false",
            "encryptionAtHost": "true",
            "securityType": "TrustedLaunch"
        },
        "sku": "2019-Datacenter",
        "storageProfile": {
            "dataDisks": [{
                "bytesPerSecondThrottle": "979202048",
                "caching": "None",
                "createOption": "Empty",
                "diskCapacityBytes": "274877906944",
                "diskSizeGB": "1024",
                "image": {
                  "uri": ""
                },
                "isSharedDisk": "false",
                "isUltraDisk": "true",
                "lun": "0",
                "managedDisk": {
                  "id": "/subscriptions/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx/resourceGroups/macikgo-test-may-23/providers/Microsoft.Compute/disks/exampledatadiskname",
                  "storageAccountType": "StandardSSD_LRS"
                },
                "name": "exampledatadiskname",
                "opsPerSecondThrottle": "65280",
                "vhd": {
                  "uri": ""
                },
                "writeAcceleratorEnabled": "false"
            }],
            "imageReference": {
                "id": "",
                "offer": "WindowsServer",
                "publisher": "MicrosoftWindowsServer",
                "sku": "2019-Datacenter",
                "version": "latest",
                "communityGalleryImageId": "/CommunityGalleries/testgallery/Images/1804Gen2/Versions/latest",
                "sharedGalleryImageId": "/SharedGalleries/1P/Images/gen2/Versions/latest",
                "exactVersion": "1.1686127202.30113"
            },
            "osDisk": {
                "caching": "ReadWrite",
                "createOption": "FromImage",
                "diskSizeGB": "30",
                "diffDiskSettings": {
                    "option": "Local"
                },
                "encryptionSettings": {
                  "enabled": "false",
                  "diskEncryptionKey": {
                    "sourceVault": {
                      "id": "/subscriptions/test-source-guid/resourceGroups/testrg/providers/Microsoft.KeyVault/vaults/test-kv"
                    },
                    "secretUrl": "https://test-disk.vault.azure.net/secrets/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx"
                  },
                  "keyEncryptionKey": {
                    "sourceVault": {
                      "id": "/subscriptions/test-key-guid/resourceGroups/testrg/providers/Microsoft.KeyVault/vaults/test-kv"
                    },
                    "keyUrl": "https://test-key.vault.azure.net/secrets/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx"
                  }
                },
                "image": {
                    "uri": ""
                },
                "managedDisk": {
                    "id": "/subscriptions/xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx/resourceGroups/macikgo-test-may-23/providers/Microsoft.Compute/disks/exampleosdiskname",
                    "storageAccountType": "StandardSSD_LRS"
                },
                "name": "exampleosdiskname",
                "osType": "Windows",
                "vhd": {
                    "uri": ""
                },
                "writeAcceleratorEnabled": "false"
            },
            "resourceDisk": {
                "size": "4096"
            }
        },
        "subscriptionId": "xxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxx",
        "tags": "baz:bash;foo:bar",
        "userData": "Zm9vYmFy",
        "version": "15.05.22",
        "virtualMachineScaleSet": {
            "id": "/subscriptions/xxxxxxxx-xxxxx-xxx-xxx-xxxx/resourceGroups/resource-group-name/providers/Microsoft.Compute/virtualMachineScaleSets/virtual-machine-scale-set-name"
        },
        "vmId": "02aab8a4-74ef-476e-8182-f6d2ba4166a6",
        "vmScaleSetName": "crpteste9vflji9",
        "vmSize": "Standard_A3",
        "zone": ""
    },
    "network": {
        "interface": [{
            "ipv4": {
               "ipAddress": [{
                    "privateIpAddress": "10.144.133.132",
                    "publicIpAddress": ""
                }],
                "subnet": [{
                    "address": "10.144.133.128",
                    "prefix": "26"
                }]
            },
            "ipv6": {
                "ipAddress": [
                 ]
            },
            "macAddress": "0011AAFFBB22"
        }]
    }
}
"#;
}
