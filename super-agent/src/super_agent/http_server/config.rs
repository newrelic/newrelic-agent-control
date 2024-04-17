use serde::Deserialize;
use std::fmt::{Display, Formatter};

const DEFAULT_PORT: u16 = 51200;
const DEFAULT_WORKERS: usize = 1;
const DEFAULT_HOST: &str = "127.0.0.1";

#[derive(PartialEq, Deserialize, Debug, Clone)]
pub struct Port(u16);
#[derive(PartialEq, Deserialize, Debug, Clone)]
pub struct Workers(usize);
#[derive(PartialEq, Deserialize, Debug, Clone)]
pub struct Host(String);

#[derive(PartialEq, Deserialize, Clone, Debug, Default)]
pub struct ServerConfig {
    #[serde(default)]
    pub port: Port,
    #[serde(default)]
    pub workers: Workers,
    #[serde(default)]
    pub host: Host,
    #[serde(default)]
    pub enabled: bool,
}

impl Default for Port {
    fn default() -> Self {
        Port(DEFAULT_PORT)
    }
}

impl From<Port> for u16 {
    fn from(value: Port) -> Self {
        value.0
    }
}

impl Default for Workers {
    fn default() -> Self {
        Workers(DEFAULT_WORKERS)
    }
}

impl From<Workers> for usize {
    fn from(value: Workers) -> Self {
        value.0
    }
}

impl Default for Host {
    fn default() -> Self {
        Host(String::from(DEFAULT_HOST))
    }
}

impl Display for Port {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl Display for Workers {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl Display for Host {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod test {
    use crate::super_agent::http_server::config::{
        Host, Port, ServerConfig, Workers, DEFAULT_HOST, DEFAULT_PORT, DEFAULT_WORKERS,
    };
    use serde::Deserialize;

    #[derive(Deserialize, Default, Debug, PartialEq)]
    struct ConfigContainer {
        #[serde(default)]
        server_config: ServerConfig,
    }

    #[test]
    fn test_deserialize_default() {
        struct Test {
            content: String,
            expected: ConfigContainer,
        }
        impl Test {
            fn run(&self) {
                let config: ConfigContainer = serde_yaml::from_str(&self.content).unwrap();
                assert_eq!(self.expected, config);
            }
        }

        let tests: Vec<Test> = vec![
            Test {
                content: String::from(r#""#),
                expected: ConfigContainer {
                    server_config: ServerConfig {
                        host: Host(String::from(DEFAULT_HOST)),
                        port: Port(DEFAULT_PORT),
                        workers: Workers(DEFAULT_WORKERS),
                        enabled: false,
                    },
                },
            },
            Test {
                content: String::from(
                    r#"
                        server_config:
                          host: 192.168.1.10
                          workers: 5
                          enabled: true
                        "#,
                ),
                expected: ConfigContainer {
                    server_config: ServerConfig {
                        host: Host(String::from("192.168.1.10")),
                        port: Port(DEFAULT_PORT),
                        workers: Workers(5),
                        enabled: true,
                    },
                },
            },
            Test {
                content: String::from(
                    r#"
                        server_config:
                          host: 192.168.1.10
                          port: 4321
                        "#,
                ),
                expected: ConfigContainer {
                    server_config: ServerConfig {
                        host: Host(String::from("192.168.1.10")),
                        port: Port(4321),
                        workers: Workers(1),
                        enabled: false,
                    },
                },
            },
            Test {
                content: String::from(
                    r#"
                        server_config:
                          workers: 192
                          port: 4321
                        "#,
                ),
                expected: ConfigContainer {
                    server_config: ServerConfig {
                        host: Host(String::from("127.0.0.1")),
                        port: Port(4321),
                        workers: Workers(192),
                        enabled: false,
                    },
                },
            },
        ];

        tests.iter().for_each(|t| t.run());
    }
}
