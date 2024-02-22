pub(super) const AWS_VM_RESPONSE: &str = r#"
{
  "accountId": "012345678901",
  "architecture": "x86_64",
  "availabilityZone": "eu-central-1c",
  "billingProducts": null,
  "devpayProductCodes": null,
  "marketplaceProductCodes": null,
  "imageId": "ami-01ff76477b9b30d59",
  "instanceId": "i-123456787d725bbe7",
  "instanceType": "t3a.nano",
  "kernelId": null,
  "pendingTime": "2022-06-20T09:51:52Z",
  "privateIp": "172.29.40.136",
  "ramdiskId": null,
  "region": "eu-central-1",
  "version": "2017-09-30"
}
"#;

pub(super) const GCP_VM_RESPONSE: &str = r#"
{
		"attributes": {},
		"cpuPlatform": "Intel Haswell",
		"description": "",
		"disks": [
			{
				"deviceName": "mmacias-micro",
				"index": 0,
				"mode": "READ_WRITE",
				"type": "PERSISTENT"
			}
		],
		"hostname": "mmacias-micro.c.beyond-181918.internal",
		"id": 6331980990053453154,
		"image": "projects/debian-cloud/global/images/debian-9-stretch-v20171025",
		"licenses": [
			{
				"id": "1000205"
			}
		],
		"machineType": "projects/260890654058/machineTypes/f1-micro",
		"maintenanceEvent": "NONE",
		"name": "mmacias-micro",
		"networkInterfaces": [
			{
				"accessConfigs": [
					{
						"externalIp": "104.154.137.202",
						"type": "ONE_TO_ONE_NAT"
					}
				],
				"forwardedIps": [],
				"ip": "10.128.0.5",
				"ipAliases": [],
				"mac": "42:01:0a:80:00:05",
				"network": "projects/260890654058/networks/default",
				"targetInstanceIps": []
			}
		],
		"preempted": "FALSE",
		"scheduling": {
			"automaticRestart": "TRUE",
			"onHostMaintenance": "MIGRATE",
			"preemptible": "FALSE"
		},
		"serviceAccounts": {},
		"tags": [],
		"virtualClock": {
			"driftToken": "0"
		},
		"zone": "projects/260890654058/zones/us-central1-c"
	}
"#;

pub(super) const AZURE_VM_RESPONSE: &str = r#"
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
