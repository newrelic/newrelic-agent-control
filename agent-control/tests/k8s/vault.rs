use crate::common::runtime::block_on;
use crate::k8s::tools::k8s_env::K8sEnv;
use newrelic_agent_control::secrets_provider::SecretsProvider;
use newrelic_agent_control::secrets_provider::vault::{Vault, VaultConfig, VaultSecretPath};
use serde_json::Value;
use std::fs::File;
use std::io::Read;

const VAULT_CONFIG: &str = r#"
sources:
  sourceA:
    url: http://127.0.0.1:8200/v1/
    token: root
    engine: kv1
  sourceB:
    url: http://127.0.0.1:8200/v1/
    token: root
    engine: kv2
client_timeout: 3s
"#;

const KV1_SOURCE: &str = "sourceA";
const KV2_SOURCE: &str = "sourceB";

// These mounts and path come defined in the tests TiltFile when creating the test secrets.
const KV1_MOUNT: &str = "kv-v1";
const KV2_MOUNT: &str = "secret";
const PATH: &str = "my-secret";

// Data files used to create the vault kv1 and kv2 secrets in the TiltFile
const KV1_DATA_PATH: &str = "tests/k8s/data/vault_kv1_secrets.json";
const KV2_DATA_PATH: &str = "tests/k8s/data/vault_kv2_secrets.json";

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_vault_get_secrets() {
    // start the port_forwarder that will allow accessing vault in port 8200
    let mut k8s = block_on(K8sEnv::new());
    k8s.port_forward("vault-0", 8200, 8200);

    let vault_config = serde_yaml::from_str::<VaultConfig>(VAULT_CONFIG).unwrap();
    let vault_client = Vault::try_build(vault_config).unwrap();

    let mut file_kv1 = File::open(KV1_DATA_PATH).expect("Failed to open KV1 data file");
    let mut data_kv1 = String::new();
    file_kv1
        .read_to_string(&mut data_kv1)
        .expect("Failed to read KV1 data file");

    // Parse the string of data into a serde_json::Value
    let parsed: Value = serde_json::from_str(&data_kv1).expect("Failed to parse JSON data");
    if let Value::Object(map) = parsed {
        for (key, value) in map.iter() {
            let vault_secret_path = VaultSecretPath {
                source: KV1_SOURCE.to_string(),
                mount: KV1_MOUNT.to_string(),
                path: PATH.to_string(),
                name: key.to_string(),
            };
            assert_eq!(
                vault_client.get_secret(vault_secret_path).unwrap(),
                value.clone()
            );
        }
    }

    let mut file_kv2 = File::open(KV2_DATA_PATH).expect("Failed to open KV2 data file");
    let mut data_kv2 = String::new();
    file_kv2
        .read_to_string(&mut data_kv2)
        .expect("Failed to read KV2 data file");

    // Parse the string of data into a serde_json::Value
    let parsed: Value = serde_json::from_str(&data_kv2).expect("Failed to parse JSON data");
    if let Value::Object(map) = parsed {
        if let Some(Value::Object(data_map)) = map.get("data") {
            for (key, value) in data_map.iter() {
                let vault_secret_path = VaultSecretPath {
                    source: KV2_SOURCE.to_string(),
                    mount: KV2_MOUNT.to_string(),
                    path: PATH.to_string(),
                    name: key.to_string(),
                };
                assert_eq!(
                    vault_client.get_secret(vault_secret_path).unwrap(),
                    value.clone()
                );
            }
        }
    }
}
